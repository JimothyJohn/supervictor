use crate::error::CliError;
use crate::preflight::{check_docker_running, require};
use crate::runner::mock::MockRunner;
use crate::runner::CommandOutput;

#[test]
fn test_check_docker_running_success() {
    let runner = MockRunner::new();
    runner.push_result(CommandOutput {
        status: 0,
        stdout: "Docker info output".to_string(),
        stderr: String::new(),
    });
    assert!(check_docker_running(&runner));
}

#[test]
fn test_check_docker_running_failure() {
    let runner = MockRunner::new();
    runner.push_result(CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: "Cannot connect to Docker".to_string(),
    });
    assert!(!check_docker_running(&runner));
}

#[test]
fn test_require_docker_not_running() {
    let runner = MockRunner::new();
    runner.push_result(CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: "Cannot connect".to_string(),
    });

    // Use tools we know exist (sh is always on PATH)
    let result = require(&["sh"], true, &runner);
    assert!(matches!(result, Err(CliError::DockerNotRunning)));
}

#[test]
fn test_require_all_present_docker_up() {
    let runner = MockRunner::new();
    runner.push_result(CommandOutput {
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    });

    let result = require(&["sh"], true, &runner);
    assert!(result.is_ok());
}

#[test]
fn test_require_missing_tool() {
    let runner = MockRunner::new();

    let result = require(&["nonexistent_tool_xyz_12345"], false, &runner);
    assert!(matches!(result, Err(CliError::MissingTools(_))));
}
