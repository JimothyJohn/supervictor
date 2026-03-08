pub mod certs;
pub mod flash;
pub mod preflight;
pub mod register;
pub mod server;
pub mod verify;

use std::path::PathBuf;
use std::process::{Child, Command};

use crate::config::ProjectConfig;
use crate::error::CliError;
use crate::output;
use crate::runner::Runner;

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseStatus {
    Passed,
    Failed,
    Skipped,
}

pub struct PhaseResult {
    pub status: PhaseStatus,
    pub message: String,
}

impl PhaseResult {
    pub fn passed() -> Self {
        Self { status: PhaseStatus::Passed, message: String::new() }
    }
    pub fn failed(msg: impl Into<String>) -> Self {
        Self { status: PhaseStatus::Failed, message: msg.into() }
    }
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self { status: PhaseStatus::Skipped, message: msg.into() }
    }
}

pub struct OnboardContext<'a> {
    pub config: &'a ProjectConfig,
    pub runner: &'a dyn Runner,
    pub device_name: String,
    pub owner_id: String,
    pub mode: String, // "onprem" | "aws"
    pub verbose: bool,
    pub dry_run: bool,
    // Populated by phases:
    pub certs_dir: Option<PathBuf>,
    pub subject_dn: Option<String>,
    pub api_url: Option<String>,
    pub api_process: Option<Child>,
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

pub struct OnboardArgs {
    pub device_name: String,
    pub owner_id: String,
    pub mode: String,
    pub verbose: bool,
    pub dry_run: bool,
    pub start_at: usize,
    pub skip: Vec<usize>,
}

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
            output::step(&format!("Skipping phase {}: {} (--start-at {})", i, name, start_at));
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
