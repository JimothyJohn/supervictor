use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn unique_path(name: &str) -> std::path::PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("qs_test_env_{}_{}", name, n))
}

#[test]
fn test_load_env_basic() {
    let path = unique_path("basic");
    write_file(&path, "FOO=bar\nBAZ=qux\n");

    let map = qs::env::load_env(&path).unwrap();
    assert_eq!(map.get("FOO").unwrap(), "bar");
    assert_eq!(map.get("BAZ").unwrap(), "qux");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_load_env_missing_file() {
    let result = qs::env::load_env(std::path::Path::new("/nonexistent/.env"));
    assert!(result.is_err());
}

#[test]
fn test_load_env_comments_and_blanks() {
    let path = unique_path("comments");
    write_file(&path, "# comment\n\nFOO=bar\n  # another\n");

    let map = qs::env::load_env(&path).unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(map.get("FOO").unwrap(), "bar");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_load_env_quoted() {
    let path = unique_path("quoted");
    write_file(&path, "A=\"hello world\"\nB='single'\n");

    let map = qs::env::load_env(&path).unwrap();
    assert_eq!(map.get("A").unwrap(), "hello world");
    assert_eq!(map.get("B").unwrap(), "single");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_load_env_equals_in_value() {
    let path = unique_path("equals");
    write_file(&path, "URL=https://example.com?a=1&b=2\n");

    let map = qs::env::load_env(&path).unwrap();
    assert_eq!(map.get("URL").unwrap(), "https://example.com?a=1&b=2");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_make_env_merges() {
    let mut overrides = std::collections::HashMap::new();
    overrides.insert("TEST_QS_KEY".to_string(), "test_value".to_string());

    let env = qs::env::make_env(&overrides);
    assert_eq!(env.get("TEST_QS_KEY").unwrap(), "test_value");
    assert!(env.contains_key("PATH"));
}

fn write_file(path: &std::path::Path, contents: &str) {
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
}
