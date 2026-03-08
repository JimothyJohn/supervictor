use crate::error::CliError;
use crate::runner::{RunOptions, Runner};

/// Get the host target triple from `rustc -vV` (e.g. "aarch64-apple-darwin").
pub fn host_target(runner: &dyn Runner) -> Result<String, CliError> {
    let opts = RunOptions {
        capture: true,
        ..Default::default()
    };
    let output = runner.run(&["rustc", "-vV"], &opts)?;

    for line in output.stdout.lines() {
        if let Some(triple) = line.strip_prefix("host: ") {
            return Ok(triple.trim().to_string());
        }
    }

    Err(CliError::Config(
        "could not determine host target from `rustc -vV`".to_string(),
    ))
}
