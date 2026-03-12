use std::io::Write;
use std::path::Path;

use crate::commands::prod::{run_prod, ProdArgs};
use crate::config::ProjectConfig;
use crate::runner::mock::MockRunner;
use crate::runner::CommandOutput;

fn setup(tmp: &Path) -> ProjectConfig {
    for name in [".env.dev", ".env.staging", ".env.prod"] {
        let path = tmp.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "HOST=localhost").unwrap();
    }

    std::fs::create_dir_all(tmp.join(".logs")).unwrap();

    let mut cfg = ProjectConfig::from_repo_root(tmp);
    cfg.env_dev = tmp.join(".env.dev");
    cfg.env_staging = tmp.join(".env.staging");
    cfg.env_prod = tmp.join(".env.prod");
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
fn dev_gate_failure_aborts_prod() {
    let tmp = std::env::temp_dir().join("qs_prod_dev_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let r = MockRunner::new();
    // dev: docker info ok, rustc ok, cargo test fails
    r.push_result(ok());
    r.push_result(rustc_output());
    r.push_result(fail());

    let args = ProdArgs {
        verbose: false,
        dry_run: false,
    };
    let code = run_prod(&args, &cfg, &r).unwrap();
    assert_eq!(code, 1);

    // No staging or deploy calls after dev failure
    let calls = r.calls.borrow();
    assert!(!calls.iter().any(|c| c.contains(&"deploy".to_string())));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn dev_gate_endpoint_failure_aborts_prod() {
    let tmp = std::env::temp_dir().join("qs_prod_endpoint_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let r = MockRunner::new();
    // dev gate: docker, rustc, device tests pass, endpoint tests fail
    r.push_result(ok()); // docker info
    r.push_result(rustc_output()); // rustc -vV
    r.push_result(ok()); // device tests
    r.push_result(fail()); // endpoint tests fail

    let args = ProdArgs {
        verbose: false,
        dry_run: false,
    };
    let code = run_prod(&args, &cfg, &r).unwrap();
    assert_eq!(code, 1);

    let _ = std::fs::remove_dir_all(&tmp);
}
