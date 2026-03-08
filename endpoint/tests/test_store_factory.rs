use supervictor_endpoint::config::Config;
use supervictor_endpoint::store::factory::create_store;

fn test_config(backend: &str) -> Config {
    Config {
        environment: "test".into(),
        app_name: "supervictor".into(),
        log_level: "error".into(),
        port: 8000,
        store_backend: backend.into(),
        devices_table: "devices".into(),
        messages_table: "messages".into(),
        sqlite_db_path: ":memory:".into(),
    }
}

#[test]
fn create_sqlite_store() {
    let config = test_config("sqlite");
    let store = create_store(&config);
    assert!(store.is_ok());
}

#[test]
fn unknown_backend_errors() {
    let config = test_config("redis");
    let result = create_store(&config);
    match result {
        Err(e) => {
            let msg = format!("{e}");
            assert!(msg.contains("unknown store backend"), "got: {msg}");
        }
        Ok(_) => panic!("expected error for unknown backend"),
    }
}
