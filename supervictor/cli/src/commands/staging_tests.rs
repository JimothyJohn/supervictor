use std::io::Write;
use std::path::Path;

use crate::commands::staging::{run_staging, StagingArgs};
use crate::config::ProjectConfig;
use crate::runner::mock::MockRunner;
use crate::runner::CommandOutput;

fn setup(tmp: &Path) -> ProjectConfig {
    let env_dev = tmp.join(".env.dev");
    let mut f = std::fs::File::create(&env_dev).unwrap();
    writeln!(f, "HOST=localhost").unwrap();

    let env_staging = tmp.join(".env.staging");
    let mut f = std::fs::File::create(&env_staging).unwrap();
    writeln!(f, "HOST=staging.example.com").unwrap();

    std::fs::create_dir_all(tmp.join(".logs")).unwrap();

    let mut cfg = ProjectConfig::from_repo_root(tmp);
    cfg.env_dev = env_dev;
    cfg.env_staging = env_staging;
    cfg
}

fn ok() -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn fail() -> CommandOutput {
    CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: "failed".to_string(),
    }
}

fn rustc_output() -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: "rustc 1.78.0\nbinary: rustc\nhost: aarch64-apple-darwin\n".to_string(),
        stderr: String::new(),
    }
}

#[test]
fn dev_gate_failure_aborts_staging() {
    let tmp = std::env::temp_dir().join("qs_staging_dev_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let r = MockRunner::new();
    // dev gate: docker info ok, rustc ok, cargo test (device) fails
    r.push_result(ok());
    r.push_result(rustc_output());
    r.push_result(fail());

    let args = StagingArgs {
        verbose: false,
        dry_run: false,
    };
    let code = run_staging(&args, &cfg, &r, false).unwrap();
    assert_eq!(code, 1);

    // No sam or staging calls after dev failure
    let calls = r.calls.borrow();
    assert!(!calls
        .iter()
        .any(|c| c.contains(&"sam".to_string()) && c.contains(&"build".to_string())));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn dev_gate_endpoint_tests_fail_aborts_staging() {
    let tmp = std::env::temp_dir().join("qs_staging_endpoint_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let r = MockRunner::new();
    // dev gate: docker, rustc, device tests pass, endpoint tests fail
    r.push_result(ok());
    r.push_result(rustc_output());
    r.push_result(ok()); // device tests pass
    r.push_result(fail()); // endpoint tests fail

    let args = StagingArgs {
        verbose: false,
        dry_run: false,
    };
    let code = run_staging(&args, &cfg, &r, false).unwrap();
    assert_eq!(code, 1);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn skip_dev_gate_runs_preflight() {
    let tmp = std::env::temp_dir().join("qs_staging_skip_dev");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let r = MockRunner::new();
    // Two calls bypass dry_run (they construct their own RunOptions):
    //  1. check_docker_running → docker info (needs status: 0)
    //  2. host_target → rustc -vV (needs "host: ..." in stdout)
    r.push_result(ok());
    r.push_result(rustc_output());

    let args = StagingArgs {
        verbose: false,
        dry_run: true,
    };
    let code = run_staging(&args, &cfg, &r, true).unwrap();
    assert_eq!(code, 0);

    // Should have called sam build and deploy (even in dry-run, calls are recorded)
    let calls = r.calls.borrow();
    assert!(calls
        .iter()
        .any(|c| c.contains(&"sam".to_string()) && c.contains(&"build".to_string())));
    assert!(calls
        .iter()
        .any(|c| c.contains(&"sam".to_string()) && c.contains(&"deploy".to_string())));

    let _ = std::fs::remove_dir_all(&tmp);
}
