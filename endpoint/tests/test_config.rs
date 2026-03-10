use supervictor_endpoint::config::Config;

/// Env var tests are inherently sequential — process-global state.
/// Combined into one test to avoid races between parallel threads.
#[test]
fn env_var_parsing() {
    // Save and clear all relevant vars
    let keys = [
        "PORT",
        "AWS_LWA_PORT",
        "ENVIRONMENT",
        "APP_NAME",
        "LOG_LEVEL",
        "STORE_BACKEND",
        "DEVICES_TABLE",
        "MESSAGES_TABLE",
        "SQLITE_DB_PATH",
    ];
    let saved: Vec<_> = keys.iter().map(|k| std::env::var(k).ok()).collect();
    for k in &keys {
        std::env::remove_var(k);
    }

    // --- defaults ---
    let cfg = Config::from_env().unwrap();
    assert_eq!(cfg.port, 8000);
    assert_eq!(cfg.environment, "dev");
    assert_eq!(cfg.app_name, "supervictor");
    assert_eq!(cfg.log_level, "info");
    assert_eq!(cfg.store_backend, "sqlite");
    assert_eq!(cfg.devices_table, "devices");
    assert_eq!(cfg.messages_table, "messages");
    assert_eq!(cfg.sqlite_db_path, ":memory:");

    // --- PORT override ---
    std::env::set_var("PORT", "3000");
    assert_eq!(Config::from_env().unwrap().port, 3000);
    std::env::remove_var("PORT");

    // --- AWS_LWA_PORT fallback ---
    std::env::set_var("AWS_LWA_PORT", "9000");
    assert_eq!(Config::from_env().unwrap().port, 9000);

    // --- PORT takes precedence ---
    std::env::set_var("PORT", "3000");
    assert_eq!(Config::from_env().unwrap().port, 3000);
    std::env::remove_var("PORT");
    std::env::remove_var("AWS_LWA_PORT");

    // --- invalid port ---
    std::env::set_var("PORT", "not_a_number");
    let err = Config::from_env().unwrap_err();
    assert!(format!("{err}").contains("not_a_number"));
    std::env::remove_var("PORT");

    // --- port overflow ---
    std::env::set_var("PORT", "99999");
    assert!(Config::from_env().is_err());
    std::env::remove_var("PORT");

    // --- custom fields ---
    std::env::set_var("ENVIRONMENT", "production");
    assert_eq!(Config::from_env().unwrap().environment, "production");
    std::env::remove_var("ENVIRONMENT");

    std::env::set_var("STORE_BACKEND", "dynamo");
    assert_eq!(Config::from_env().unwrap().store_backend, "dynamo");
    std::env::remove_var("STORE_BACKEND");

    // Restore original env
    for (k, v) in keys.iter().zip(saved.iter()) {
        match v {
            Some(val) => std::env::set_var(k, val),
            None => std::env::remove_var(k),
        }
    }
}
