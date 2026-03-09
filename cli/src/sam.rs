use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use crate::config::ProjectConfig;
use crate::error::CliError;
use crate::runner::{self, BackgroundOptions, RunOptions, Runner};

/// Lambda env var overrides for sam local (no DynamoDB available locally).
const LOCAL_ENV_OVERRIDES: &str = r#"{"HelloWorldFunction":{"STORE_BACKEND":"sqlite"}}"#;

/// SAM local lifecycle manager.
pub struct SamLocal<'a> {
    config: &'a ProjectConfig,
    env: Option<HashMap<String, String>>,
    verbose: bool,
    dry_run: bool,
    proc: Option<Child>,
    env_file: Option<PathBuf>,
}

/// Guard returned by `start()` that terminates the process on drop.
pub struct SamGuard {
    proc: Option<Child>,
    env_file: Option<PathBuf>,
}

impl Drop for SamGuard {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.proc {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(ref path) = self.env_file {
            let _ = fs::remove_file(path);
        }
    }
}

impl SamGuard {
    /// Take ownership of the child process, preventing the guard from killing it on drop.
    pub fn take_process(&mut self) -> Option<Child> {
        self.proc.take()
    }

    /// Block until the process exits.
    pub fn wait(&mut self) -> Result<(), CliError> {
        if let Some(ref mut child) = self.proc {
            child.wait()?;
        }
        Ok(())
    }

    /// Detach the process so it keeps running after this guard is dropped.
    /// Writes the PID to `pid_file` for later cleanup.
    pub fn detach(mut self, pid_file: &std::path::Path) -> Result<u32, CliError> {
        let child = self
            .proc
            .take()
            .ok_or_else(|| CliError::Config("no SAM process to detach (dry-run?)".to_string()))?;
        let pid = child.id();

        if let Some(parent) = pid_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(pid_file, pid.to_string())?;

        // Leak the Child so Drop doesn't kill it
        std::mem::forget(child);
        Ok(pid)
    }
}

/// Kill a previously-detached SAM process using its PID file.
/// Returns Ok(true) if the process was killed, Ok(false) if it wasn't running.
pub fn stop_from_pid_file(pid_file: &std::path::Path) -> Result<bool, CliError> {
    let contents = match fs::read_to_string(pid_file) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(e) => return Err(e.into()),
    };

    let pid = contents.trim().to_string();
    if pid.is_empty() {
        let _ = fs::remove_file(pid_file);
        return Ok(false);
    }

    // Use kill(1) to send SIGTERM — no libc dependency needed
    let status = std::process::Command::new("kill")
        .arg(&pid)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let killed = status.is_ok_and(|s| s.success());

    // Clean up PID file regardless
    let _ = fs::remove_file(pid_file);

    Ok(killed)
}

impl<'a> SamLocal<'a> {
    pub fn new(
        config: &'a ProjectConfig,
        env: Option<HashMap<String, String>>,
        verbose: bool,
        dry_run: bool,
    ) -> Self {
        Self {
            config,
            env,
            verbose,
            dry_run,
            proc: None,
            env_file: None,
        }
    }

    pub fn url(&self) -> String {
        format!("http://localhost:{}", self.config.sam_local_port)
    }

    /// Export runtime deps and run sam build.
    pub fn build(&self, r: &dyn Runner, no_cache: bool) -> Result<(), CliError> {
        let log_dir = &self.config.log_dir;

        runner::step("Exporting runtime dependencies");
        r.run(
            &[
                "uv",
                "export",
                "--no-dev",
                "--no-hashes",
                "-o",
                "requirements.txt",
            ],
            &RunOptions {
                cwd: Some(self.config.cloud_dir.join("uplink")),
                env: self.env.clone(),
                verbose: self.verbose,
                dry_run: self.dry_run,
                log_to: Some(log_dir.join("uv_export.log")),
                ..Default::default()
            },
        )?;

        runner::step("Building SAM artifacts");
        let mut cmd: Vec<&str> = vec!["sam", "build", "--skip-pull-image"];
        if no_cache {
            cmd.push("--no-cached");
        }
        r.run(
            &cmd,
            &RunOptions {
                cwd: Some(self.config.cloud_dir.clone()),
                env: self.env.clone(),
                verbose: self.verbose,
                dry_run: self.dry_run,
                log_to: Some(log_dir.join("sam_build.log")),
                ..Default::default()
            },
        )?;
        runner::success("SAM build complete");
        Ok(())
    }

    /// Start sam local start-api in background and wait for readiness.
    /// Returns a guard that stops the process on drop.
    pub fn start(&mut self, r: &dyn Runner, extra_args: &[&str]) -> Result<SamGuard, CliError> {
        runner::step(&format!(
            "Starting sam local on port {}",
            self.config.sam_local_port
        ));

        let env_file = self.write_env_overrides()?;
        let env_file_str = env_file.to_string_lossy().to_string();
        let port_str = self.config.sam_local_port.to_string();

        let mut cmd = vec![
            "sam",
            "local",
            "start-api",
            "--port",
            &port_str,
            "--skip-pull-image",
            "--env-vars",
            &env_file_str,
        ];
        cmd.extend_from_slice(extra_args);

        let child = r.start_background(
            &cmd,
            &BackgroundOptions {
                cwd: Some(self.config.cloud_dir.clone()),
                env: self.env.clone(),
                log_file: Some(PathBuf::from(&self.config.sam_log_file)),
                verbose: self.verbose,
                dry_run: self.dry_run,
            },
        )?;

        self.env_file = Some(env_file.clone());
        self.proc = child;

        Ok(SamGuard {
            proc: self.proc.take(),
            env_file: Some(env_file),
        })
    }

    /// Poll until sam local's HTTP server is up and responding.
    pub fn wait_ready(&self) -> Result<(), CliError> {
        if self.dry_run {
            println!("  [dry-run] wait for sam local ready");
            return Ok(());
        }

        let health_url = format!("{}/health", self.url());
        println!("  Waiting for sam local at {} ...", self.url());
        let deadline = Instant::now() + Duration::from_secs(self.config.sam_ready_timeout);

        while Instant::now() < deadline {
            // Full HTTP probe — TCP alone succeeds before Lambda container is ready
            // Accept any HTTP response (even 403) — we just need the server responding
            match ureq::get(&health_url).call() {
                Ok(_) | Err(ureq::Error::StatusCode(_)) => {
                    runner::success("  sam local ready.");
                    return Ok(());
                }
                Err(_) => {}
            }
            std::thread::sleep(Duration::from_secs(1));
        }

        Err(CliError::Timeout {
            message: format!(
                "sam local did not start within {}s. Check logs: {}",
                self.config.sam_ready_timeout,
                self.config.sam_log_file.display()
            ),
        })
    }

    /// Read stack_name from env vars, falling back to samconfig.toml.
    fn read_stack_name(&self, config_env: &str) -> Result<String, CliError> {
        let env = self.env.as_ref();
        if let Some(name) = env.and_then(|e| e.get("SAM_STACK_NAME")) {
            if !name.is_empty() {
                return Ok(name.clone());
            }
        }

        let samconfig = self.config.cloud_dir.join("samconfig.toml");
        let contents = fs::read_to_string(&samconfig).map_err(|e| {
            CliError::Config(format!("failed to read {}: {}", samconfig.display(), e))
        })?;
        let data: toml::Value = contents
            .parse()
            .map_err(|e| CliError::Config(format!("failed to parse samconfig.toml: {}", e)))?;

        // Try deploy.parameters first, then global.parameters
        let paths = [&["deploy", "parameters"][..], &["global", "parameters"][..]];
        for path in &paths {
            let mut section = data.get(config_env);
            for key in *path {
                section = section.and_then(|s| s.get(key));
            }
            if let Some(name) = section.and_then(|s| s.get("stack_name")) {
                if let Some(s) = name.as_str() {
                    return Ok(s.trim_matches('"').to_string());
                }
            }
        }

        Err(CliError::Config(format!(
            "no stack_name found in samconfig.toml for config-env '{}'",
            config_env
        )))
    }

    /// Query CloudFormation for the deployed API endpoint URL.
    pub fn stack_endpoint(&self, r: &dyn Runner, config_env: &str) -> Result<String, CliError> {
        if self.dry_run {
            return Ok(format!(
                "https://DRY-RUN.execute-api.us-east-1.amazonaws.com/{}",
                config_env
            ));
        }

        let stack_name = self.read_stack_name(config_env)?;
        let result = r.run(
            &[
                "aws",
                "cloudformation",
                "describe-stacks",
                "--stack-name",
                &stack_name,
                "--query",
                "Stacks[0].Outputs[?OutputKey=='SupervictorApiEndpoint'].OutputValue",
                "--output",
                "text",
            ],
            &RunOptions {
                env: self.env.clone(),
                verbose: self.verbose,
                capture: true,
                ..Default::default()
            },
        )?;

        let url = result.stdout.trim().trim_end_matches('/').to_string();
        if url.is_empty() {
            return Err(CliError::Config(format!(
                "no SupervictorApiEndpoint output found for stack '{}'",
                stack_name
            )));
        }
        Ok(url)
    }

    /// Run sam deploy. Returns true if changes were deployed.
    pub fn deploy(
        &self,
        r: &dyn Runner,
        config_env: &str,
        force_upload: bool,
    ) -> Result<bool, CliError> {
        let log_path = self
            .config
            .log_dir
            .join(format!("sam_deploy_{}.log", config_env));
        runner::step(&format!("Deploying to {} stack", config_env));

        let mut cmd_parts: Vec<String> = vec![
            "sam".into(),
            "deploy".into(),
            "--config-env".into(),
            config_env.into(),
        ];

        // Override samconfig values from env vars
        let env = self.env.as_ref();
        if let Some(v) = env
            .and_then(|e| e.get("SAM_STACK_NAME"))
            .filter(|v| !v.is_empty())
        {
            cmd_parts.extend(["--stack-name".into(), v.clone()]);
        }
        if let Some(v) = env
            .and_then(|e| e.get("SAM_REGION"))
            .filter(|v| !v.is_empty())
        {
            cmd_parts.extend(["--region".into(), v.clone()]);
        }
        if let Some(v) = env
            .and_then(|e| e.get("SAM_S3_PREFIX"))
            .filter(|v| !v.is_empty())
        {
            cmd_parts.extend(["--s3-prefix".into(), v.clone()]);
        }

        // Build --parameter-overrides
        let param_map = [
            ("SAM_ENVIRONMENT", "Environment"),
            ("SAM_APP_NAME", "AppName"),
            ("SAM_STACK_NAME", "StackName"),
            ("SAM_TRUSTSTORE_URI", "TruststoreUri"),
        ];
        let mut param_parts: Vec<String> = Vec::new();
        for (env_key, cfn_param) in &param_map {
            if let Some(val) = env.and_then(|e| e.get(*env_key)).filter(|v| !v.is_empty()) {
                param_parts.push(format!("{}={}", cfn_param, val));
            }
        }
        if !param_parts.is_empty() {
            cmd_parts.push("--parameter-overrides".into());
            cmd_parts.push(param_parts.join(" "));
        }

        if force_upload {
            cmd_parts.push("--force-upload".into());
        }

        let cmd_refs: Vec<&str> = cmd_parts.iter().map(|s| s.as_str()).collect();
        let result = r.run(
            &cmd_refs,
            &RunOptions {
                cwd: Some(self.config.cloud_dir.clone()),
                env: self.env.clone(),
                verbose: self.verbose,
                dry_run: self.dry_run,
                check: false,
                log_to: Some(log_path.clone()),
                ..Default::default()
            },
        )?;

        if result.status != 0 {
            let combined = format!("{}{}", result.stdout, result.stderr);
            if combined.contains("No changes to deploy") {
                runner::success("Stack is already up to date.");
                return Ok(false);
            }
            runner::error(&format!("sam deploy failed (see {})", log_path.display()));
            return Err(CliError::Command {
                cmd: cmd_parts.join(" "),
                code: result.status,
                stderr: result.stderr,
            });
        }

        runner::success(&format!("Deployed to {}", config_env));
        Ok(true)
    }

    fn write_env_overrides(&self) -> Result<PathBuf, CliError> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("sam_env_{}.json", std::process::id()));
        fs::write(&path, LOCAL_ENV_OVERRIDES)?;
        Ok(path)
    }
}

#[cfg(test)]
#[path = "sam_tests.rs"]
mod tests;
