use std::time::{Duration, Instant};

use crate::error::CliError;
use crate::runner::{self, CommandOutput, RunOptions, Runner};

const TRUSTSTORE_DOMAIN: &str = "supervictor.advin.io";
const TRUSTSTORE_BUCKET: &str = "supervictor";
const TRUSTSTORE_KEY: &str = "truststore.pem";
const TRUSTSTORE_TEMP_KEY: &str = "truststore-reload.pem";

const RETRIABLE_ERRORS: &[&str] = &[
    "TooManyRequests",
    "Throttling",
    "ThrottlingException",
    "RequestLimitExceeded",
    "ConflictException",
];

const RETRY_DELAY: Duration = Duration::from_secs(3);
const MAX_RETRIES: u32 = 3;
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const POLL_TIMEOUT: Duration = Duration::from_secs(120);

/// Force API Gateway to re-read the mTLS truststore from S3.
///
/// API Gateway ignores `update-domain-name` when the URI hasn't changed,
/// so we swap to a temp copy, wait for the domain to settle, then swap back.
pub fn reload(r: &dyn Runner, verbose: bool, dry_run: bool) -> Result<(), CliError> {
    runner::step("Reloading API Gateway mTLS truststore");
    if dry_run {
        println!("  [dry-run] truststore reload skipped");
        return Ok(());
    }

    let uri = format!("s3://{TRUSTSTORE_BUCKET}/{TRUSTSTORE_KEY}");
    let temp_uri = format!("s3://{TRUSTSTORE_BUCKET}/{TRUSTSTORE_TEMP_KEY}");
    let opts = RunOptions {
        check: false,
        capture: true,
        verbose,
        ..Default::default()
    };

    // Step 1: Copy truststore to temp key
    let cp = r.run(&["aws", "s3", "cp", &uri, &temp_uri], &opts)?;
    if cp.status != 0 {
        runner::error(&format!("Truststore copy failed: {}", cp.stderr.trim()));
        return Ok(());
    }

    // From here on, always clean up the temp key
    let result = do_swap_restore(r, &uri, &temp_uri, &opts);

    // Step 6: Clean up temp key (always, even on failure)
    let _ = r.run(&["aws", "s3", "rm", &temp_uri], &opts);

    result
}

/// Swap domain to temp URI, wait, restore, and verify.
/// Separated so cleanup runs unconditionally in the caller.
fn do_swap_restore(
    r: &dyn Runner,
    uri: &str,
    temp_uri: &str,
    opts: &RunOptions,
) -> Result<(), CliError> {
    let patch_temp =
        format!("op=replace,path=/mutualTlsAuthentication/truststoreUri,value={temp_uri}");
    let patch_canonical =
        format!("op=replace,path=/mutualTlsAuthentication/truststoreUri,value={uri}");

    // Step 2: Point domain to temp URI
    let swap = aws_retry(
        r,
        &[
            "aws",
            "apigateway",
            "update-domain-name",
            "--domain-name",
            TRUSTSTORE_DOMAIN,
            "--patch-operations",
            &patch_temp,
        ],
        opts,
    )?;
    if swap.status != 0 {
        runner::error(&format!("Truststore swap failed: {}", swap.stderr.trim()));
        return Ok(());
    }

    // Step 3: Wait for domain to finish processing the swap
    if !wait_domain_available(r, opts)? {
        runner::error("Timed out waiting for domain to become AVAILABLE after swap");
        return Ok(());
    }

    // Step 4: Point domain back to canonical URI
    let restore = aws_retry(
        r,
        &[
            "aws",
            "apigateway",
            "update-domain-name",
            "--domain-name",
            TRUSTSTORE_DOMAIN,
            "--patch-operations",
            &patch_canonical,
        ],
        opts,
    )?;
    if restore.status != 0 {
        runner::error(&format!(
            "Truststore restore failed: {}",
            restore.stderr.trim()
        ));
        return Ok(());
    }

    // Step 5: Verify final state
    let verify = r.run(
        &[
            "aws",
            "apigateway",
            "get-domain-name",
            "--domain-name",
            TRUSTSTORE_DOMAIN,
            "--query",
            "mutualTlsAuthentication.truststoreUri",
            "--output",
            "text",
        ],
        opts,
    )?;
    let actual = verify.stdout.trim().to_string();
    if verify.status == 0 && !actual.is_empty() && actual != uri {
        runner::error(&format!("Truststore URI mismatch after reload: {actual}"));
        return Ok(());
    }

    runner::success("mTLS truststore reloaded");
    Ok(())
}

/// Run an AWS CLI command with retries on retriable errors.
fn aws_retry(r: &dyn Runner, cmd: &[&str], opts: &RunOptions) -> Result<CommandOutput, CliError> {
    let mut result = r.run(cmd, opts)?;

    for _ in 0..MAX_RETRIES {
        if result.status == 0 || !is_retriable(&result.stderr) {
            break;
        }
        runner::step(&format!(
            "Retriable error, retrying in {}s...",
            RETRY_DELAY.as_secs()
        ));
        std::thread::sleep(RETRY_DELAY);
        result = r.run(cmd, opts)?;
    }

    Ok(result)
}

fn is_retriable(stderr: &str) -> bool {
    RETRIABLE_ERRORS.iter().any(|e| stderr.contains(e))
}

/// Poll `get-domain-name` until `domainNameStatus` is `AVAILABLE`.
fn wait_domain_available(r: &dyn Runner, opts: &RunOptions) -> Result<bool, CliError> {
    let deadline = Instant::now() + POLL_TIMEOUT;

    while Instant::now() < deadline {
        let result = r.run(
            &[
                "aws",
                "apigateway",
                "get-domain-name",
                "--domain-name",
                TRUSTSTORE_DOMAIN,
                "--query",
                "domainNameStatus",
                "--output",
                "text",
            ],
            opts,
        )?;

        if result.status == 0 {
            let status = result.stdout.trim();
            if status == "AVAILABLE" {
                return Ok(true);
            }
            runner::step(&format!("Domain status: {status}, waiting..."));
        }

        std::thread::sleep(POLL_INTERVAL);
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::mock::MockRunner;

    fn ok(stdout: &str) -> CommandOutput {
        CommandOutput {
            status: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn fail(stderr: &str) -> CommandOutput {
        CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn happy_path_calls_all_six_steps() {
        let r = MockRunner::new();
        r.push_result(ok("")); // s3 cp
        r.push_result(ok("")); // swap
        r.push_result(ok("AVAILABLE\n")); // poll
        r.push_result(ok("")); // restore
        r.push_result(ok("s3://supervictor/truststore.pem\n")); // verify
        r.push_result(ok("")); // cleanup

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        assert_eq!(r.call_count(), 6);

        assert!(r.get_call(0).contains(&"cp".to_string()));
        assert!(r.get_call(1).contains(&"update-domain-name".to_string()));
        assert!(r.get_call(2).contains(&"get-domain-name".to_string()));
        assert!(r.get_call(3).contains(&"update-domain-name".to_string()));
        assert!(r.get_call(4).contains(&"get-domain-name".to_string()));
        assert!(r.get_call(5).contains(&"rm".to_string()));
    }

    #[test]
    fn dry_run_makes_no_calls() {
        let r = MockRunner::new();
        let result = reload(&r, false, true);
        assert!(result.is_ok());
        assert_eq!(r.call_count(), 0);
    }

    #[test]
    fn copy_failure_aborts_without_cleanup() {
        let r = MockRunner::new();
        r.push_result(fail("access denied"));

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        // Only the s3 cp call — no temp key was created
        assert_eq!(r.call_count(), 1);
    }

    #[test]
    fn swap_failure_still_cleans_up_temp() {
        let r = MockRunner::new();
        r.push_result(ok("")); // s3 cp
        r.push_result(fail("bad request")); // swap fails
        r.push_result(ok("")); // s3 rm (cleanup)

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        assert_eq!(r.call_count(), 3);
        assert!(r.get_call(2).contains(&"rm".to_string()));
    }

    #[test]
    fn restore_failure_still_cleans_up_temp() {
        let r = MockRunner::new();
        r.push_result(ok("")); // s3 cp
        r.push_result(ok("")); // swap
        r.push_result(ok("AVAILABLE\n")); // poll
        r.push_result(fail("InternalFailure")); // restore fails
        r.push_result(ok("")); // s3 rm (cleanup)

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        assert_eq!(r.call_count(), 5);
        assert!(r.get_call(4).contains(&"rm".to_string()));
    }

    #[test]
    fn conflict_exception_is_retried() {
        let r = MockRunner::new();
        r.push_result(ok("")); // s3 cp
        r.push_result(fail("ConflictException: domain update in progress")); // swap fail
        r.push_result(ok("")); // swap retry ok
        r.push_result(ok("AVAILABLE\n")); // poll
        r.push_result(ok("")); // restore
        r.push_result(ok("s3://supervictor/truststore.pem\n")); // verify
        r.push_result(ok("")); // cleanup

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        assert_eq!(r.call_count(), 7);
    }

    #[test]
    fn throttling_exception_is_retried() {
        let r = MockRunner::new();
        r.push_result(ok("")); // s3 cp
        r.push_result(fail("ThrottlingException: Rate exceeded")); // swap fail
        r.push_result(ok("")); // swap retry ok
        r.push_result(ok("AVAILABLE\n")); // poll
        r.push_result(ok("")); // restore
        r.push_result(ok("s3://supervictor/truststore.pem\n")); // verify
        r.push_result(ok("")); // cleanup

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        assert_eq!(r.call_count(), 7);
    }

    #[test]
    fn retries_exhausted_on_persistent_throttle() {
        let r = MockRunner::new();
        r.push_result(ok("")); // s3 cp
        r.push_result(fail("TooManyRequestsException")); // swap: initial
        r.push_result(fail("TooManyRequestsException")); // retry 1
        r.push_result(fail("TooManyRequestsException")); // retry 2
        r.push_result(fail("TooManyRequestsException")); // retry 3
        r.push_result(ok("")); // cleanup

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        // s3 cp (1) + swap attempts (4) + cleanup (1) = 6
        assert_eq!(r.call_count(), 6);
    }

    #[test]
    fn uri_mismatch_after_restore_reports_error() {
        let r = MockRunner::new();
        r.push_result(ok("")); // s3 cp
        r.push_result(ok("")); // swap
        r.push_result(ok("AVAILABLE\n")); // poll
        r.push_result(ok("")); // restore
        r.push_result(ok("s3://supervictor/truststore-reload.pem\n")); // verify: wrong URI
        r.push_result(ok("")); // cleanup

        let result = reload(&r, false, false);
        assert!(result.is_ok());
        assert_eq!(r.call_count(), 6);
    }
}
