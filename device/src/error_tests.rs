use super::HttpError;
use core::fmt::Write;
use heapless::String as HString;

#[test]
fn deserialization_variant_exists() {
    let _e = HttpError::Deserialization;
}

#[test]
fn generic_parse_error_variant_exists() {
    let _e = HttpError::GenericParseError;
}

#[test]
fn deserialization_display_message() {
    let e = HttpError::Deserialization;
    let mut buf: HString<64> = HString::new();
    write!(buf, "{}", e).unwrap();
    assert_eq!(buf.as_str(), "Failed to deserialize response");
}

#[test]
fn generic_parse_error_display_message() {
    let e = HttpError::GenericParseError;
    let mut buf: HString<64> = HString::new();
    write!(buf, "{}", e).unwrap();
    assert_eq!(buf.as_str(), "Failed to parse response");
}

#[test]
fn display_messages_are_distinct() {
    let mut buf_a: HString<64> = HString::new();
    let mut buf_b: HString<64> = HString::new();
    write!(buf_a, "{}", HttpError::Deserialization).unwrap();
    write!(buf_b, "{}", HttpError::GenericParseError).unwrap();
    assert_ne!(buf_a.as_str(), buf_b.as_str());
}

#[test]
fn display_messages_are_non_empty() {
    let mut buf: HString<64> = HString::new();
    write!(buf, "{}", HttpError::Deserialization).unwrap();
    assert!(!buf.is_empty());

    buf.clear();
    write!(buf, "{}", HttpError::GenericParseError).unwrap();
    assert!(!buf.is_empty());
}
