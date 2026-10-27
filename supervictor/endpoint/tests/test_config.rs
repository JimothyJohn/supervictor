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
        "REQUIRE_REGISTRATION",
        "POWER_THRESHOLD_MA",
        "TLS_CERT",
        "TLS_KEY",
        "TLS_CLIENT_CA",
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
    assert!(!cfg.require_registration, "open ingest by default");
    assert_eq!(
        cfg.power_threshold_ma, 1000,
        "default power threshold: the clamp floor"
    );

    // --- POWER_THRESHOLD_MA: any integer, whitespace tolerated, junk fails boot ---
    for (raw, expected) in [("250", 250), (" 0 ", 0), ("-5", -5)] {
        std::env::set_var("POWER_THRESHOLD_MA", raw);
        assert_eq!(
            Config::from_env().unwrap().power_threshold_ma,
            expected,
            "POWER_THRESHOLD_MA={raw:?}"
        );
    }
    for bad in ["lots", "1.5", "100mA", ""] {
        std::env::set_var("POWER_THRESHOLD_MA", bad);
        let err = Config::from_env().unwrap_err();
        assert!(
            format!("{err}").contains("POWER_THRESHOLD_MA"),
            "POWER_THRESHOLD_MA={bad:?}: {err}"
        );
    }
    std::env::remove_var("POWER_THRESHOLD_MA");

    // --- REQUIRE_REGISTRATION: accepted spellings ---
    for (raw, expected) in [
        ("true", true),
        ("TRUE", true),
        ("1", true),
        ("yes", true),
        ("on", true),
        ("false", false),
        ("0", false),
        ("no", false),
        ("off", false),
        ("", false),
    ] {
        std::env::set_var("REQUIRE_REGISTRATION", raw);
        assert_eq!(
            Config::from_env().unwrap().require_registration,
            expected,
            "REQUIRE_REGISTRATION={raw:?}"
        );
    }
    // A typo in a security knob fails the boot instead of silently
    // disabling the check.
    std::env::set_var("REQUIRE_REGISTRATION", "ture");
    let err = Config::from_env().unwrap_err();
    assert!(
        format!("{err}").contains("REQUIRE_REGISTRATION"),
        "got: {err}"
    );
    std::env::remove_var("REQUIRE_REGISTRATION");

    // --- TLS: all or nothing ---
    assert!(
        Config::from_env().unwrap().tls.is_none(),
        "plain HTTP by default"
    );
    std::env::set_var("TLS_CERT", "/etc/supervictor/server.pem");
    std::env::set_var("TLS_KEY", "/etc/supervictor/server.key");
    std::env::set_var("TLS_CLIENT_CA", "/etc/supervictor/ca.pem");
    let tls = Config::from_env().unwrap().tls.expect("all three set");
    assert_eq!(tls.cert.to_str(), Some("/etc/supervictor/server.pem"));
    assert_eq!(tls.key.to_str(), Some("/etc/supervictor/server.key"));
    assert_eq!(tls.client_ca.to_str(), Some("/etc/supervictor/ca.pem"));
    // A partial set must not silently fall back to plaintext.
    std::env::remove_var("TLS_CLIENT_CA");
    let err = Config::from_env().unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("TLS_CLIENT_CA"), "got: {msg}");
    assert!(
        !msg.contains("TLS_CERT,"),
        "only the missing one is named: {msg}"
    );
    std::env::set_var("TLS_CERT", "   ");
    let msg = format!("{}", Config::from_env().unwrap_err());
    assert!(
        msg.contains("TLS_CERT") && msg.contains("TLS_CLIENT_CA"),
        "got: {msg}"
    );
    for k in ["TLS_CERT", "TLS_KEY", "TLS_CLIENT_CA"] {
        std::env::remove_var(k);
    }

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
