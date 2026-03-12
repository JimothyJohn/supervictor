use std::path::{Path, PathBuf};

/// Project-wide configuration. Mirrors quickstart/config.py.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    /// Absolute path to the repository root.
    pub repo_root: PathBuf,
    /// Path to the `supervictor/edge/` crate directory.
    pub edge_dir: PathBuf,
    /// Path to the `supervictor/endpoint/` crate directory.
    pub endpoint_dir: PathBuf,
    /// Path to the `.env.dev` file.
    pub env_dev: PathBuf,
    /// Path to the `.env.staging` file.
    pub env_staging: PathBuf,
    /// Path to the `.env.prod` file.
    pub env_prod: PathBuf,
    /// Path to the `.logs/` directory for build and test output.
    pub log_dir: PathBuf,

    /// Port for `sam local start-api`.
    pub sam_local_port: u16,
    /// Seconds to wait for SAM local readiness.
    pub sam_ready_timeout: u64,
    /// Path to the SAM local log file.
    pub sam_log_file: PathBuf,
    /// Path to the SAM local PID file (for `--serve` / `--stop`).
    pub sam_pid_file: PathBuf,
    /// SAM config-env name for the dev stack.
    pub sam_config_env_dev: String,
    /// SAM config-env name for the prod stack.
    pub sam_config_env_prod: String,

    /// Directory name for certificates (relative to repo root).
    pub certs_dir_name: String,
    /// Relative path to the cert generation script.
    pub gen_certs_script: String,
    /// Production API endpoint URL.
    pub prod_api_endpoint: String,
}

impl ProjectConfig {
    /// Build a config from the given repository root, deriving all paths.
    pub fn from_repo_root(root: &Path) -> Self {
        let root = root.to_path_buf();
        let log_dir = root.join(".logs");
        let sv = root.join("supervictor");
        Self {
            edge_dir: sv.join("edge"),
            endpoint_dir: sv.join("endpoint"),
            env_dev: root.join(".env.dev"),
            env_staging: root.join(".env.staging"),
            env_prod: root.join(".env.prod"),
            sam_log_file: log_dir.join("sam_local.log"),
            sam_pid_file: log_dir.join("sam_local.pid"),
            log_dir,
            repo_root: root,

            sam_local_port: 3000,
            sam_ready_timeout: 120,
            sam_config_env_dev: "dev".to_string(),
            sam_config_env_prod: "prod".to_string(),
            certs_dir_name: "certs".to_string(),
            gen_certs_script: "scripts/gen_certs.sh".to_string(),
            prod_api_endpoint: "https://supervictor.advin.io".to_string(),
        }
    }

    /// Absolute path to the certificates directory.
    pub fn certs_dir(&self) -> PathBuf {
        self.repo_root.join(&self.certs_dir_name)
    }

    /// Absolute path to the `gen_certs.sh` script.
    pub fn gen_certs_script_path(&self) -> PathBuf {
        self.endpoint_dir.join(&self.gen_certs_script)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_from_repo_root() {
        let cfg = ProjectConfig::from_repo_root(Path::new("/tmp/repo"));
        assert_eq!(cfg.repo_root, Path::new("/tmp/repo"));
        assert_eq!(cfg.edge_dir, Path::new("/tmp/repo/supervictor/edge"));
        assert_eq!(cfg.endpoint_dir, Path::new("/tmp/repo/supervictor/endpoint"));
        assert_eq!(cfg.env_dev, Path::new("/tmp/repo/.env.dev"));
        assert_eq!(cfg.log_dir, Path::new("/tmp/repo/.logs"));
        assert_eq!(cfg.sam_local_port, 3000);
        assert_eq!(cfg.sam_ready_timeout, 120);
    }

    #[test]
    fn test_certs_dir() {
        let cfg = ProjectConfig::from_repo_root(Path::new("/tmp/repo"));
        assert_eq!(cfg.certs_dir(), Path::new("/tmp/repo/certs"));
    }

    #[test]
    fn test_gen_certs_script_path() {
        let cfg = ProjectConfig::from_repo_root(Path::new("/tmp/repo"));
        assert_eq!(
            cfg.gen_certs_script_path(),
            Path::new("/tmp/repo/supervictor/endpoint/scripts/gen_certs.sh")
        );
    }
}
