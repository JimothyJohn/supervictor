use std::path::{Path, PathBuf};

/// Project-wide configuration. Mirrors quickstart/config.py.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub repo_root: PathBuf,
    pub device_dir: PathBuf,
    pub endpoint_dir: PathBuf,
    pub env_dev: PathBuf,
    pub env_staging: PathBuf,
    pub env_prod: PathBuf,
    pub log_dir: PathBuf,

    pub sam_local_port: u16,
    pub sam_ready_timeout: u64,
    pub sam_log_file: PathBuf,
    pub sam_pid_file: PathBuf,
    pub sam_config_env_dev: String,
    pub sam_config_env_prod: String,

    pub certs_dir_name: String,
    pub gen_certs_script: String,
    pub prod_api_endpoint: String,
}

impl ProjectConfig {
    pub fn from_repo_root(root: &Path) -> Self {
        let root = root.to_path_buf();
        let log_dir = root.join(".logs");
        Self {
            device_dir: root.join("device"),
            endpoint_dir: root.join("endpoint"),
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

    pub fn certs_dir(&self) -> PathBuf {
        self.repo_root.join(&self.certs_dir_name)
    }

    pub fn gen_certs_script_path(&self) -> PathBuf {
        self.repo_root.join("cloud").join(&self.gen_certs_script)
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
        assert_eq!(cfg.device_dir, Path::new("/tmp/repo/device"));
        assert_eq!(cfg.endpoint_dir, Path::new("/tmp/repo/endpoint"));
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
            Path::new("/tmp/repo/cloud/scripts/gen_certs.sh")
        );
    }
}
