use std::path::Path;

use qs::config::ProjectConfig;

#[test]
fn test_from_repo_root_paths() {
    let cfg = ProjectConfig::from_repo_root(Path::new("/tmp/repo"));
    assert_eq!(cfg.repo_root, Path::new("/tmp/repo"));
    assert_eq!(cfg.edge_dir, Path::new("/tmp/repo/supervictor/edge"));
    assert_eq!(cfg.env_dev, Path::new("/tmp/repo/.env.dev"));
    assert_eq!(cfg.env_staging, Path::new("/tmp/repo/.env.staging"));
    assert_eq!(cfg.env_prod, Path::new("/tmp/repo/.env.prod"));
    assert_eq!(cfg.log_dir, Path::new("/tmp/repo/.logs"));
}

#[test]
fn test_defaults() {
    let cfg = ProjectConfig::from_repo_root(Path::new("/tmp/repo"));
    assert_eq!(cfg.sam_local_port, 3000);
    assert_eq!(cfg.sam_ready_timeout, 120);
    assert_eq!(cfg.sam_config_env_dev, "dev");
    assert_eq!(cfg.sam_config_env_prod, "prod");
    assert_eq!(cfg.certs_dir_name, "certs");
    assert_eq!(cfg.prod_api_endpoint, "https://supervictor.advin.io");
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
