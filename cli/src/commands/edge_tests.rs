use std::io::Write;
use std::path::Path;

use crate::commands::edge::{run_edge, EdgeArgs};
use crate::config::ProjectConfig;
use crate::runner::mock::MockRunner;
use crate::runner::CommandOutput;

fn setup(tmp: &Path) -> ProjectConfig {
    let env_path = tmp.join(".env.dev");
    let mut f = std::fs::File::create(&env_path).unwrap();
    writeln!(f, "HOST=localhost").unwrap();

    let mut cfg = ProjectConfig::from_repo_root(tmp);
    cfg.env_dev = env_path;
    cfg
}

fn ok() -> CommandOutput {
    CommandOutput::default()
}

#[test]
fn test_edge_happy_path() {
    let tmp = std::env::temp_dir().join("qs_edge_happy");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let runner = MockRunner::new();
    runner.push_result(ok()); // cargo run (flash)

    let args = EdgeArgs {
        verbose: false,
        dry_run: false,
    };
    let code = run_edge(&args, &cfg, &runner).unwrap();
    assert_eq!(code, 0);

    let call = runner.get_call(0);
    assert!(call.contains(&"cargo".to_string()));
    assert!(call.contains(&"run".to_string()));
    assert!(call.contains(&"supervictor-embedded".to_string()));
    assert!(call.contains(&"embedded".to_string()));
    // No --port args when ESPFLASH_PORT is not set
    assert!(!call.contains(&"--port".to_string()));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_edge_with_port_env() {
    let tmp = std::env::temp_dir().join("qs_edge_port");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Write .env.dev with ESPFLASH_PORT
    let env_path = tmp.join(".env.dev");
    let mut f = std::fs::File::create(&env_path).unwrap();
    writeln!(f, "HOST=localhost").unwrap();
    writeln!(f, "ESPFLASH_PORT=/dev/ttyUSB0").unwrap();

    let mut cfg = ProjectConfig::from_repo_root(&tmp);
    cfg.env_dev = env_path;

    let runner = MockRunner::new();
    runner.push_result(ok());

    let args = EdgeArgs {
        verbose: false,
        dry_run: false,
    };
    let code = run_edge(&args, &cfg, &runner).unwrap();
    assert_eq!(code, 0);

    let call = runner.get_call(0);
    assert!(call.contains(&"--port".to_string()));
    assert!(call.contains(&"/dev/ttyUSB0".to_string()));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_edge_flash_failure_returns_1() {
    let tmp = std::env::temp_dir().join("qs_edge_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = setup(&tmp);

    let runner = MockRunner::new();
    runner.push_result(CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: "flash failed".to_string(),
    });

    let args = EdgeArgs {
        verbose: false,
        dry_run: false,
    };
    let code = run_edge(&args, &cfg, &runner).unwrap();
    assert_eq!(code, 1);

    let _ = std::fs::remove_dir_all(&tmp);
}
