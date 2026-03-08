use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::error::CliError;
use crate::output;

/// Output captured from a completed subprocess.
#[derive(Debug, Clone, Default)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Options controlling how a command is executed.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub cwd: Option<PathBuf>,
    pub env: Option<HashMap<String, String>>,
    pub check: bool,
    pub capture: bool,
    pub verbose: bool,
    pub dry_run: bool,
    pub log_to: Option<PathBuf>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            cwd: None,
            env: None,
            check: true,
            capture: false,
            verbose: false,
            dry_run: false,
            log_to: None,
        }
    }
}

/// Options for starting a background process.
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct BackgroundOptions {
    pub cwd: Option<PathBuf>,
    pub env: Option<HashMap<String, String>>,
    pub log_file: Option<PathBuf>,
    pub verbose: bool,
    pub dry_run: bool,
}


/// Trait abstracting subprocess execution for testability.
pub trait Runner {
    fn run(&self, cmd: &[&str], opts: &RunOptions) -> Result<CommandOutput, CliError>;
    fn start_background(
        &self,
        cmd: &[&str],
        opts: &BackgroundOptions,
    ) -> Result<Option<Child>, CliError>;
}

/// Real subprocess runner using std::process::Command.
pub struct RealRunner;

impl Runner for RealRunner {
    fn run(&self, cmd: &[&str], opts: &RunOptions) -> Result<CommandOutput, CliError> {
        let cmd_str = cmd.join(" ");

        if opts.dry_run {
            println!("  [dry-run] {}", cmd_str);
            return Ok(CommandOutput::default());
        }

        if opts.verbose {
            println!("  $ {}", cmd_str);
        }

        let (program, args) = cmd
            .split_first()
            .ok_or_else(|| CliError::Config("empty command".to_string()))?;

        let mut command = Command::new(program);
        command.args(args);

        if let Some(cwd) = &opts.cwd {
            command.current_dir(cwd);
        }
        if let Some(env) = &opts.env {
            command.envs(env);
        }

        // Route based on log_to vs capture vs passthrough
        if let Some(log_path) = &opts.log_to {
            return run_with_log(&mut command, &cmd_str, log_path, opts);
        }

        if opts.capture {
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());
        }

        let output = command.output()?;
        let result = CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        };

        if opts.check && result.status != 0 {
            return Err(CliError::Command {
                cmd: cmd_str,
                code: result.status,
                stderr: result.stderr.clone(),
            });
        }

        Ok(result)
    }

    fn start_background(
        &self,
        cmd: &[&str],
        opts: &BackgroundOptions,
    ) -> Result<Option<Child>, CliError> {
        let cmd_str = cmd.join(" ");

        if opts.dry_run {
            println!("  [dry-run] {} &", cmd_str);
            return Ok(None);
        }

        if opts.verbose {
            println!("  $ {} &", cmd_str);
        }

        let (program, args) = cmd
            .split_first()
            .ok_or_else(|| CliError::Config("empty command".to_string()))?;

        let mut command = Command::new(program);
        command.args(args);

        if let Some(cwd) = &opts.cwd {
            command.current_dir(cwd);
        }
        if let Some(env) = &opts.env {
            command.envs(env);
        }

        if let Some(log_path) = &opts.log_file {
            if let Some(parent) = log_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let log = fs::File::create(log_path)?;
            command.stdout(log.try_clone()?);
            command.stderr(log);
        } else {
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
        }

        let child = command.spawn()?;
        Ok(Some(child))
    }
}

fn run_with_log(
    command: &mut Command,
    cmd_str: &str,
    log_path: &Path,
    opts: &RunOptions,
) -> Result<CommandOutput, CliError> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}{}", stdout, stderr);

    fs::write(log_path, &combined)?;

    if opts.verbose {
        print!("{}", combined);
    }

    let status = output.status.code().unwrap_or(-1);
    if opts.check && status != 0 {
        return Err(CliError::Command {
            cmd: cmd_str.to_string(),
            code: status,
            stderr: stderr.clone(),
        });
    }

    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

/// Convenience: print a step header.
pub fn step(msg: &str) {
    output::step(msg);
}

/// Convenience: print a milestone header.
pub fn milestone(msg: &str) {
    output::milestone(msg);
}

/// Convenience: print a success message.
pub fn success(msg: &str) {
    output::success(msg);
}

/// Convenience: print an error message.
pub fn error(msg: &str) {
    output::error(msg);
}

/// Interactive yes/no confirmation.
pub fn confirm(prompt: &str) -> bool {
    output::confirm(prompt)
}

// ── Test support ──────────────────────────────────────────────────────

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Records every call and returns pre-loaded results.
    pub struct MockRunner {
        pub calls: RefCell<Vec<Vec<String>>>,
        pub results: RefCell<VecDeque<CommandOutput>>,
        pub bg_calls: RefCell<Vec<Vec<String>>>,
    }

    impl MockRunner {
        pub fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                results: RefCell::new(VecDeque::new()),
                bg_calls: RefCell::new(Vec::new()),
            }
        }

        pub fn push_result(&self, result: CommandOutput) {
            self.results.borrow_mut().push_back(result);
        }

        pub fn call_count(&self) -> usize {
            self.calls.borrow().len()
        }

        pub fn get_call(&self, idx: usize) -> Vec<String> {
            self.calls.borrow()[idx].clone()
        }
    }

    impl Runner for MockRunner {
        fn run(&self, cmd: &[&str], opts: &RunOptions) -> Result<CommandOutput, CliError> {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();

            if opts.dry_run {
                println!("  [dry-run] {}", cmd.join(" "));
                self.calls.borrow_mut().push(cmd_vec);
                return Ok(CommandOutput::default());
            }

            self.calls.borrow_mut().push(cmd_vec);

            let result = self
                .results
                .borrow_mut()
                .pop_front()
                .unwrap_or_default();

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
            opts: &BackgroundOptions,
        ) -> Result<Option<Child>, CliError> {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();

            if opts.dry_run {
                println!("  [dry-run] {} &", cmd.join(" "));
            }

            self.bg_calls.borrow_mut().push(cmd_vec);
            Ok(None)
        }
    }
}
