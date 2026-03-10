use crate::error::CliError;
use crate::runner::{self, CommandOutput, RunOptions, Runner};

/// Pinned espflash version installed by `ensure_espflash`.
pub const ESPFLASH_VERSION: &str = "3.3.0";

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
    matches!(
        runner.run(&["docker", "info"], &opts),
        Ok(CommandOutput { status: 0, .. })
    )
}

/// Install espflash at the pinned version if it is missing or mismatched.
pub fn ensure_espflash(runner: &dyn Runner) -> Result<(), CliError> {
    let needs_install = if which("espflash").is_some() {
        // Check version
        let opts = RunOptions {
            capture: true,
            check: false,
            ..Default::default()
        };
        match runner.run(&["espflash", "--version"], &opts) {
            Ok(CommandOutput {
                status: 0, stdout, ..
            }) => {
                let installed = stdout.trim().replace("espflash ", "");
                if installed == ESPFLASH_VERSION {
                    return Ok(());
                }
                runner::step(&format!(
                    "espflash {} found, expected {} — reinstalling",
                    installed, ESPFLASH_VERSION
                ));
                true
            }
            _ => true,
        }
    } else {
        runner::step("espflash not found — installing");
        true
    };

    if needs_install {
        let pkg = format!("espflash@{}", ESPFLASH_VERSION);
        runner.run(
            &["cargo", "install", &pkg],
            &RunOptions {
                check: true,
                ..Default::default()
            },
        )?;
        runner::success(&format!("Installed espflash {}", ESPFLASH_VERSION));
    }

    Ok(())
}

/// Exit with an error if any required tools are missing or Docker is down.
pub fn require(tools: &[&str], need_docker: bool, runner: &dyn Runner) -> Result<(), CliError> {
    let missing = check_tools(tools);
    if !missing.is_empty() {
        return Err(CliError::MissingTools(missing));
    }
    if need_docker && !check_docker_running(runner) {
        return Err(CliError::DockerNotRunning);
    }
    Ok(())
}

#[cfg(test)]
#[path = "preflight_tests.rs"]
mod tests;
