use std::collections::HashMap;
use std::path::Path;

use crate::error::CliError;

/// Parse a bash-style `.env` file into a HashMap.
///
/// Handles `#` comments, `KEY=VALUE`, single- and double-quoted values,
/// and blank lines. Does not mutate the process environment.
pub fn load_env(path: &Path) -> Result<HashMap<String, String>, CliError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| CliError::Config(format!("failed to read {}: {}", path.display(), e)))?;
    Ok(parse_env(&contents))
}

fn parse_env(contents: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            let value = strip_quotes(value.trim());
            // Strip inline comments (only if unquoted)
            let value = strip_inline_comment(value);
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        return &s[1..s.len() - 1];
    }
    s
}

fn strip_inline_comment(s: &str) -> &str {
    // Only strip if there's a space before the #
    if let Some(pos) = s.find(" #") {
        s[..pos].trim_end()
    } else {
        s
    }
}

/// Merge `overrides` into a copy of the current process environment.
pub fn make_env(overrides: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    for (k, v) in overrides {
        env.insert(k.clone(), v.clone());
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let input = "FOO=bar\nBAZ=qux\n";
        let map = parse_env(input);
        assert_eq!(map.get("FOO").unwrap(), "bar");
        assert_eq!(map.get("BAZ").unwrap(), "qux");
    }

    #[test]
    fn test_parse_comments_and_blanks() {
        let input = "# comment\n\nFOO=bar\n  # another comment\n";
        let map = parse_env(input);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("FOO").unwrap(), "bar");
    }

    #[test]
    fn test_parse_quoted_values() {
        let input = "A=\"hello world\"\nB='single quoted'\n";
        let map = parse_env(input);
        assert_eq!(map.get("A").unwrap(), "hello world");
        assert_eq!(map.get("B").unwrap(), "single quoted");
    }

    #[test]
    fn test_parse_equals_in_value() {
        let input = "URL=https://example.com?a=1&b=2\n";
        let map = parse_env(input);
        assert_eq!(map.get("URL").unwrap(), "https://example.com?a=1&b=2");
    }

    #[test]
    fn test_parse_empty_value() {
        let input = "EMPTY=\n";
        let map = parse_env(input);
        assert_eq!(map.get("EMPTY").unwrap(), "");
    }

    #[test]
    fn test_inline_comment() {
        let input = "FOO=bar # this is a comment\n";
        let map = parse_env(input);
        assert_eq!(map.get("FOO").unwrap(), "bar");
    }

    #[test]
    fn test_make_env_merges() {
        let overrides = HashMap::from([("TEST_KEY".to_string(), "test_value".to_string())]);
        let env = make_env(&overrides);
        assert_eq!(env.get("TEST_KEY").unwrap(), "test_value");
        // PATH should exist from the process environment
        assert!(env.contains_key("PATH"));
    }
}
