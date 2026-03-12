/// Certificate generation phase (CA + device cert).
pub mod certs;
/// Firmware build and flash phase.
pub mod flash;
/// Preflight checks for required tools, Docker, and env files.
pub mod preflight;
/// Device registration phase (POST + verify active).
pub mod register;
/// API server startup phase (Docker Compose or SAM local).
pub mod server;
/// Uplink verification phase (poll for first device uplink).
pub mod verify;

use std::path::PathBuf;
use std::process::{Child, Command};

use crate::config::ProjectConfig;
use crate::error::CliError;
use crate::output;
use crate::runner::Runner;

// ── Types ─────────────────────────────────────────────────────────────

/// Outcome of a single onboard phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseStatus {
    /// Phase completed successfully.
    Passed,
    /// Phase encountered an error.
    Failed,
    /// Phase was not needed or explicitly skipped.
    Skipped,
}

/// Result returned by each onboard phase function.
pub struct PhaseResult {
    /// Whether the phase passed, failed, or was skipped.
    pub status: PhaseStatus,
    /// Human-readable detail (empty on success).
    pub message: String,
}

impl PhaseResult {
    /// Construct a successful result.
    pub fn passed() -> Self {
        Self {
            status: PhaseStatus::Passed,
            message: String::new(),
        }
    }
    /// Construct a failure result with an error message.
    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            status: PhaseStatus::Failed,
            message: msg.into(),
        }
    }
    /// Construct a skipped result with a reason.
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            status: PhaseStatus::Skipped,
            message: msg.into(),
        }
    }
}

/// Shared mutable state threaded through all onboard phases.
pub struct OnboardContext<'a> {
    /// Project configuration.
    pub config: &'a ProjectConfig,
    /// Subprocess runner (real or mock).
    pub runner: &'a dyn Runner,
    /// Identity name for the device being onboarded.
    pub device_name: String,
    /// Owner identifier for device registration.
    pub owner_id: String,
    /// Deployment mode: `"onprem"` (Docker Compose) or `"aws"` (SAM local).
    pub mode: String,
    /// Enable verbose output.
    pub verbose: bool,
    /// Print commands without executing.
    pub dry_run: bool,
    /// Populated by the certs phase: path to the certs directory.
    pub certs_dir: Option<PathBuf>,
    /// Populated by the certs phase: subject DN from the client cert.
    pub subject_dn: Option<String>,
    /// Populated by the server phase: base URL of the running API.
    pub api_url: Option<String>,
    /// Populated by the server phase: child process handle for cleanup.
    pub api_process: Option<Child>,
    /// Populated by the server phase (onprem): compose file for teardown.
    pub compose_file: Option<PathBuf>,
}

impl<'a> Drop for OnboardContext<'a> {
    fn drop(&mut self) {
        if let Some(ref compose_file) = self.compose_file {
            output::step("Stopping compose stack...");
            let _ = Command::new("docker")
                .args(["compose", "-f"])
                .arg(compose_file)
                .arg("down")
                .output();
        } else if let Some(ref mut proc) = self.api_process {
            output::step("Stopping API server...");
            let _ = proc.kill();
            let _ = proc.wait();
        }
    }
}

// ── Phase runner ──────────────────────────────────────────────────────

type PhaseFn = fn(&mut OnboardContext) -> PhaseResult;

const PHASES: &[(&str, PhaseFn)] = &[
    ("Preflight", preflight::run),
    ("Certificates", certs::run),
    ("Start Server", server::run),
    ("Register Device", register::run),
    ("Flash Firmware", flash::run),
    ("Verify Uplink", verify::run),
];

/// Arguments for the `qs onboard` command.
pub struct OnboardArgs {
    /// Device identity name.
    pub device_name: String,
    /// Owner identifier for registration.
    pub owner_id: String,
    /// Deployment mode: `"onprem"` or `"aws"`.
    pub mode: String,
    /// Enable verbose output.
    pub verbose: bool,
    /// Print commands without executing.
    pub dry_run: bool,
    /// Phase index to start from (skip earlier phases).
    pub start_at: usize,
    /// Phase indices to skip entirely.
    pub skip: Vec<usize>,
}

/// Run the full device onboarding sequence (preflight through verify).
pub fn run_onboard(
    args: &OnboardArgs,
    config: &ProjectConfig,
    r: &dyn Runner,
) -> Result<i32, CliError> {
    let mut ctx = OnboardContext {
        config,
        runner: r,
        device_name: args.device_name.clone(),
        owner_id: args.owner_id.clone(),
        mode: args.mode.clone(),
        verbose: args.verbose,
        dry_run: args.dry_run,
        certs_dir: None,
        subject_dn: None,
        api_url: None,
        api_process: None,
        compose_file: None,
    };

    Ok(run_phases(&mut ctx, args.start_at, &args.skip))
}

fn run_phases(ctx: &mut OnboardContext, start_at: usize, skip: &[usize]) -> i32 {
    for (i, (name, phase_fn)) in PHASES.iter().enumerate() {
        if i < start_at {
            output::step(&format!(
                "Skipping phase {}: {} (--start-at {})",
                i, name, start_at
            ));
            continue;
        }
        if skip.contains(&i) {
            output::step(&format!("Skipping phase {}: {} (--skip)", i, name));
            continue;
        }

        output::milestone(&format!("Phase {}: {}", i, name));
        let result = phase_fn(ctx);

        match result.status {
            PhaseStatus::Failed => {
                output::error(&format!("Phase {} failed: {}", i, result.message));
                return 1;
            }
            PhaseStatus::Skipped => {
                output::info(&format!("Phase {} skipped: {}", i, result.message));
            }
            PhaseStatus::Passed => {
                output::success(&format!("Phase {}: {}", i, name));
            }
        }
    }

    output::success(&format!("Onboarding complete for {}", ctx.device_name));
    0
}
