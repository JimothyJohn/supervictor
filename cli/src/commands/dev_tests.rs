use std::io::Write;
use std::path::Path;

use crate::commands::dev::{run_dev, DevArgs};
use crate::config::ProjectConfig;
use crate::runner::mock::MockRunner;
use crate::runner::CommandOutput;

/// Create a ProjectConfig pointing at a temp directory with a valid .env.dev file.
fn setup(tmp: &Path) -> ProjectConfig {
    // Write minimal .env.dev so load_env succeeds
    let env_path = tmp.join(".env.dev");
    let mut f = std::fs::File::create(&env_path).unwrap();
    writeln!(f, "HOST=localhost").unwrap();

    // Ensure .logs dir exists for log_to
    std::fs::create_dir_all(tmp.join(".logs")).unwrap();

    let mut cfg = ProjectConfig::from_repo_root(tmp);
    cfg.env_dev = env_path;
    cfg
}

fn ok() -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn rustc_output() -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: "rustc 1.78.0\nbinary: rustc\nhost: aarch64-apple-darwin\n".to_string(),
        stderr: String::new(),
    }
}

fn fail() -> CommandOutput {
    CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: "test failed".to_string(),
    }
}

#[test]
fn test_dev_happy_path() {
    let tmp = std::env::temp_dir().join("qs_dev_happy");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let runner = MockRunner::new();
    // preflight: docker info
    runner.push_result(ok());
    // rustc -vV (host_target)
    runner.push_result(rustc_output());
    // cargo test --lib
    runner.push_result(ok());
    // uv run pytest tests/unit/
    runner.push_result(ok());
    // uv export (sam build)
    runner.push_result(ok());
    // sam build
    runner.push_result(ok());
    // integration tests (uv run pytest tests/integration/)
    runner.push_result(ok());

    // dry_run skips the real TCP wait_ready poll
    let args = DevArgs {
        verbose: false,
        dry_run: true,
        serve: false,
    };
    let code = run_dev(&args, &cfg, &runner).unwrap();
    assert_eq!(code, 0);

    // Verify key calls were made (dry-run still records them)
    let calls = runner.calls.borrow();
    assert!(calls
        .iter()
        .any(|c| c.contains(&"cargo".to_string()) && c.contains(&"test".to_string())));
    assert!(calls
        .iter()
        .any(|c| c.contains(&"pytest".to_string()) && c.contains(&"tests/unit/".to_string())));
    assert!(calls
        .iter()
        .any(|c| c.contains(&"sam".to_string()) && c.contains(&"build".to_string())));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dev_rust_tests_fail_returns_1() {
    let tmp = std::env::temp_dir().join("qs_dev_rust_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let runner = MockRunner::new();
    runner.push_result(ok()); // docker info
    runner.push_result(rustc_output()); // rustc -vV
    runner.push_result(fail()); // cargo test fails

    let args = DevArgs {
        verbose: false,
        dry_run: false,
        serve: false,
    };
    let code = run_dev(&args, &cfg, &runner).unwrap();
    assert_eq!(code, 1);

    // No further calls after failure
    let calls = runner.calls.borrow();
    assert!(!calls.iter().any(|c| c.contains(&"pytest".to_string())));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dev_python_unit_tests_fail_returns_1() {
    let tmp = std::env::temp_dir().join("qs_dev_py_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let runner = MockRunner::new();
    runner.push_result(ok()); // docker info
    runner.push_result(rustc_output()); // rustc -vV
    runner.push_result(ok()); // cargo test OK
    runner.push_result(fail()); // pytest unit fails

    let args = DevArgs {
        verbose: false,
        dry_run: false,
        serve: false,
    };
    let code = run_dev(&args, &cfg, &runner).unwrap();
    assert_eq!(code, 1);

    // No sam build after failure
    let calls = runner.calls.borrow();
    assert!(!calls
        .iter()
        .any(|c| c.contains(&"sam".to_string()) && c.contains(&"build".to_string())));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dev_serve_skips_integration() {
    let tmp = std::env::temp_dir().join("qs_dev_serve");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let runner = MockRunner::new();
    runner.push_result(ok()); // docker info
    runner.push_result(rustc_output()); // rustc -vV
    runner.push_result(ok()); // cargo test
    runner.push_result(ok()); // pytest unit
    runner.push_result(ok()); // uv export
    runner.push_result(ok()); // sam build

    // dry_run skips the real TCP wait_ready poll
    let args = DevArgs {
        verbose: false,
        dry_run: true,
        serve: true,
    };

    let code = run_dev(&args, &cfg, &runner).unwrap();
    assert_eq!(code, 0);

    // sam local start-api should be in bg_calls
    let bg = runner.bg_calls.borrow();
    assert!(bg.iter().any(|c| c.contains(&"start-api".to_string())));

    // No integration test call
    let calls = runner.calls.borrow();
    assert!(!calls
        .iter()
        .any(|c| c.contains(&"tests/integration/".to_string())));

    let _ = std::fs::remove_dir_all(&tmp);
}
