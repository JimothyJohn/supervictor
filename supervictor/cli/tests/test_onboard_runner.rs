use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

use qs::commands::onboard::{self, OnboardArgs};
use qs::config::ProjectConfig;
use qs::error::CliError;
use qs::runner::{CommandOutput, RunOptions, Runner};

struct MockRunner {
    calls: RefCell<Vec<Vec<String>>>,
    results: RefCell<VecDeque<CommandOutput>>,
}

impl MockRunner {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            results: RefCell::new(VecDeque::new()),
        }
    }

    fn push_result(&self, result: CommandOutput) {
        self.results.borrow_mut().push_back(result);
    }
}

impl Runner for MockRunner {
    fn run(&self, cmd: &[&str], opts: &RunOptions) -> Result<CommandOutput, CliError> {
        let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
        self.calls.borrow_mut().push(cmd_vec);

        if opts.dry_run {
            return Ok(CommandOutput::default());
        }

        let result = self.results.borrow_mut().pop_front().unwrap_or_default();
        if opts.check && result.status != 0 {
            return Err(CliError::Command {
                cmd: cmd.join(" "),
                code: result.status,
                stderr: result.stderr.clone(),
            });
        }
        Ok(result)
    }

    fn start_background(
        &self,
        cmd: &[&str],
        _opts: &qs::runner::BackgroundOptions,
    ) -> Result<Option<std::process::Child>, CliError> {
        let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
        self.calls.borrow_mut().push(cmd_vec);
        Ok(None)
    }
}

fn setup_config() -> (PathBuf, ProjectConfig) {
    let dir = std::env::temp_dir().join(format!("qs_test_onboard_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let env_dev = dir.join(".env.dev");
    let mut f = std::fs::File::create(&env_dev).unwrap();
    writeln!(f, "HOST=localhost").unwrap();
    let cfg = ProjectConfig::from_repo_root(&dir);
    (dir, cfg)
}

#[test]
fn test_onboard_preflight_fails_missing_tools() {
    let (_dir, config) = setup_config();
    let mock = MockRunner::new();

    // docker info check will return success
    mock.push_result(CommandOutput { status: 0, ..Default::default() });

    let args = OnboardArgs {
        device_name: "test".to_string(),
        owner_id: "owner".to_string(),
        mode: "onprem".to_string(),
        verbose: false,
        dry_run: false,
        start_at: 0,
        skip: vec![],
    };

    // This will fail at preflight because espflash etc. are missing
    let rc = onboard::run_onboard(&args, &config, &mock).unwrap();
    assert_eq!(rc, 1); // Should fail at preflight
}

#[test]
fn test_onboard_skip_phases() {
    let (_dir, config) = setup_config();
    let mock = MockRunner::new();

    let args = OnboardArgs {
        device_name: "test".to_string(),
        owner_id: "owner".to_string(),
        mode: "onprem".to_string(),
        verbose: false,
        dry_run: false,
        start_at: 0,
        skip: vec![0, 1, 2, 3, 4, 5], // Skip all phases
    };

    let rc = onboard::run_onboard(&args, &config, &mock).unwrap();
    assert_eq!(rc, 0); // All skipped = success
}

#[test]
fn test_onboard_start_at() {
    let (_dir, config) = setup_config();
    let mock = MockRunner::new();

    let args = OnboardArgs {
        device_name: "test".to_string(),
        owner_id: "owner".to_string(),
        mode: "onprem".to_string(),
        verbose: false,
        dry_run: false,
        start_at: 6, // Past all phases
        skip: vec![],
    };

    let rc = onboard::run_onboard(&args, &config, &mock).unwrap();
    assert_eq!(rc, 0); // All skipped = success
}
