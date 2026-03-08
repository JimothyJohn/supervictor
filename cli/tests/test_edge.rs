use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use qs::config::ProjectConfig;
use qs::env;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn setup_config() -> (PathBuf, ProjectConfig) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("qs_test_edge_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    let env_dev = dir.join(".env.dev");
    let mut f = std::fs::File::create(&env_dev).unwrap();
    writeln!(f, "HOST=localhost\nPASSWORD=secret").unwrap();
    let cfg = ProjectConfig::from_repo_root(&dir);
    (dir, cfg)
}

#[test]
fn test_env_loading_for_edge() {
    let (_dir, config) = setup_config();
    let env_vars = env::load_env(&config.env_dev).unwrap();
    assert_eq!(env_vars.get("HOST").unwrap(), "localhost");
}

#[test]
fn test_espflash_port_from_env() {
    let (dir, config) = setup_config();
    // Write ESPFLASH_PORT to .env.dev
    let env_dev = dir.join(".env.dev");
    let mut f = std::fs::File::create(&env_dev).unwrap();
    writeln!(f, "HOST=localhost\nESPFLASH_PORT=/dev/ttyUSB0").unwrap();

    let env_vars = env::load_env(&config.env_dev).unwrap();
    let port = env_vars.get("ESPFLASH_PORT").unwrap();
    assert_eq!(port, "/dev/ttyUSB0");
}

#[test]
fn test_preflight_check_detects_missing_tools() {
    let missing = qs::preflight::check_tools(&["nonexistent_tool_xyz"]);
    assert_eq!(missing, vec!["nonexistent_tool_xyz"]);
}

#[test]
fn test_preflight_finds_existing_tools() {
    // 'sh' should exist on any Unix system
    let missing = qs::preflight::check_tools(&["sh"]);
    assert!(missing.is_empty());
}

#[test]
fn test_rust_tools_host_target() {
    // This tests the real `rustc -vV` parsing
    let r = qs::runner::RealRunner;
    let target = qs::rust_tools::host_target(&r).unwrap();
    assert!(!target.is_empty());
    // Should contain platform identifiers
    assert!(target.contains('-'), "Expected triple format: {}", target);
}
