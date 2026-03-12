use std::collections::HashMap;
use std::path::Path;

use crate::config::ProjectConfig;
use crate::runner::mock::MockRunner;
use crate::runner::CommandOutput;
use crate::sam::SamLocal;

fn test_config() -> ProjectConfig {
    ProjectConfig::from_repo_root(Path::new("/tmp/test-repo"))
}

fn ok_result() -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

#[test]
fn test_build_calls_sam_build() {
    let cfg = test_config();
    let runner = MockRunner::new();
    runner.push_result(ok_result()); // sam build

    let sam = SamLocal::new(&cfg, None, false, false);
    sam.build(&runner, false).unwrap();

    assert_eq!(runner.call_count(), 1);
    let call0 = runner.get_call(0);
    assert_eq!(call0[0], "sam");
    assert_eq!(call0[1], "build");
    assert!(!call0.contains(&"--no-cached".to_string()));
}

#[test]
fn test_build_no_cache_adds_flag() {
    let cfg = test_config();
    let runner = MockRunner::new();
    runner.push_result(ok_result());

    let sam = SamLocal::new(&cfg, None, false, false);
    sam.build(&runner, true).unwrap();

    let call0 = runner.get_call(0);
    assert!(call0.contains(&"--no-cached".to_string()));
}

#[test]
fn test_start_calls_background() {
    let cfg = test_config();
    let runner = MockRunner::new();

    let mut sam = SamLocal::new(&cfg, None, false, false);
    let _guard = sam.start(&runner, &[]).unwrap();

    let bg = runner.bg_calls.borrow();
    assert_eq!(bg.len(), 1);
    assert!(bg[0].contains(&"sam".to_string()));
    assert!(bg[0].contains(&"start-api".to_string()));
    assert!(bg[0].contains(&"3000".to_string()));
}

#[test]
fn test_deploy_success() {
    let cfg = test_config();
    let runner = MockRunner::new();
    runner.push_result(ok_result());

    let sam = SamLocal::new(&cfg, None, false, false);
    let deployed = sam.deploy(&runner, "dev", false).unwrap();
    assert!(deployed);

    let call = runner.get_call(0);
    assert!(call.contains(&"deploy".to_string()));
    assert!(call.contains(&"dev".to_string()));
}

#[test]
fn test_deploy_no_changes() {
    let cfg = test_config();
    let runner = MockRunner::new();
    runner.push_result(CommandOutput {
        status: 1,
        stdout: "No changes to deploy. Stack is up to date.".to_string(),
        stderr: String::new(),
    });

    let sam = SamLocal::new(&cfg, None, false, false);
    let deployed = sam.deploy(&runner, "dev", false).unwrap();
    assert!(!deployed);
}

#[test]
fn test_deploy_failure() {
    let cfg = test_config();
    let runner = MockRunner::new();
    runner.push_result(CommandOutput {
        status: 1,
        stdout: "CREATE_FAILED".to_string(),
        stderr: "stack creation failed".to_string(),
    });

    let sam = SamLocal::new(&cfg, None, false, false);
    let result = sam.deploy(&runner, "dev", false);
    assert!(result.is_err());
}

#[test]
fn test_deploy_with_env_overrides() {
    let cfg = test_config();
    let mut env = HashMap::new();
    env.insert("SAM_STACK_NAME".to_string(), "my-stack".to_string());
    env.insert("SAM_REGION".to_string(), "us-west-2".to_string());

    let runner = MockRunner::new();
    runner.push_result(ok_result());

    let sam = SamLocal::new(&cfg, Some(env), false, false);
    sam.deploy(&runner, "dev", false).unwrap();

    let call = runner.get_call(0);
    assert!(call.contains(&"--stack-name".to_string()));
    assert!(call.contains(&"my-stack".to_string()));
    assert!(call.contains(&"--region".to_string()));
    assert!(call.contains(&"us-west-2".to_string()));
}

#[test]
fn test_stack_endpoint_dry_run() {
    let cfg = test_config();
    let runner = MockRunner::new();

    let sam = SamLocal::new(&cfg, None, false, true);
    let url = sam.stack_endpoint(&runner, "dev").unwrap();
    assert!(url.starts_with("https://DRY-RUN"));
    assert!(url.contains("dev"));
    assert_eq!(runner.call_count(), 0);
}
