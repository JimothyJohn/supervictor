use std::path::PathBuf;

// We test the runner module via its public trait interface.
// Integration tests can only use the crate's public API,
// so we test via RealRunner for basic subprocess calls.

#[test]
fn test_run_echo() {
    // Use RealRunner with a simple command
    let r = qs::runner::RealRunner;
    let result = qs::runner::Runner::run(
        &r,
        &["echo", "hello"],
        &qs::runner::RunOptions {
            capture: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.status, 0);
    assert_eq!(result.stdout.trim(), "hello");
}

#[test]
fn test_run_dry_run() {
    let r = qs::runner::RealRunner;
    let result = qs::runner::Runner::run(
        &r,
        &["this-command-does-not-exist"],
        &qs::runner::RunOptions {
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(result.status, 0);
    assert_eq!(result.stdout, "");
}

#[test]
fn test_run_check_failure() {
    let r = qs::runner::RealRunner;
    let result = qs::runner::Runner::run(
        &r,
        &["false"],
        &qs::runner::RunOptions {
            check: true,
            ..Default::default()
        },
    );

    assert!(result.is_err());
}

#[test]
fn test_run_no_check_failure() {
    let r = qs::runner::RealRunner;
    let result = qs::runner::Runner::run(
        &r,
        &["false"],
        &qs::runner::RunOptions {
            check: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert_ne!(result.status, 0);
}

#[test]
fn test_run_with_cwd() {
    let r = qs::runner::RealRunner;
    let result = qs::runner::Runner::run(
        &r,
        &["pwd"],
        &qs::runner::RunOptions {
            cwd: Some(PathBuf::from("/tmp")),
            capture: true,
            ..Default::default()
        },
    )
    .unwrap();

    // /tmp may resolve to /private/tmp on macOS
    assert!(result.stdout.trim().ends_with("/tmp"));
}

#[test]
fn test_run_with_log_to() {
    let tmp = std::env::temp_dir().join("qs_test_log.txt");
    let _ = std::fs::remove_file(&tmp);

    let r = qs::runner::RealRunner;
    qs::runner::Runner::run(
        &r,
        &["echo", "logged"],
        &qs::runner::RunOptions {
            log_to: Some(tmp.clone()),
            ..Default::default()
        },
    )
    .unwrap();

    let contents = std::fs::read_to_string(&tmp).unwrap();
    assert!(contents.contains("logged"));
    let _ = std::fs::remove_file(&tmp);
}
