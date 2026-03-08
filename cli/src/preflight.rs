use crate::error::CliError;
use crate::runner::{CommandOutput, RunOptions, Runner};

/// Return the names of any tools not found on PATH.
pub fn check_tools(required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|tool| which(tool).is_none())
        .map(|s| s.to_string())
        .collect()
}

fn which(tool: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(tool))
        .find(|candidate| candidate.is_file())
}

/// Check whether the Docker daemon is responsive.
pub fn check_docker_running(runner: &dyn Runner) -> bool {
    let opts = RunOptions {
        capture: true,
        check: false,
        ..Default::default()
    };
    matches!(runner.run(&["docker", "info"], &opts), Ok(CommandOutput { status: 0, .. }))
}

/// Exit with an error if any required tools are missing or Docker is down.
pub fn require(
    tools: &[&str],
    need_docker: bool,
    runner: &dyn Runner,
) -> Result<(), CliError> {
    let missing = check_tools(tools);
    if !missing.is_empty() {
        return Err(CliError::MissingTools(missing));
    }
    if need_docker && !check_docker_running(runner) {
        return Err(CliError::DockerNotRunning);
    }
    Ok(())
}
