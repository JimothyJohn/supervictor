use axum::http::HeaderMap;
use supervictor_endpoint::middleware::extract_client_subject;

#[test]
fn extract_from_lwa_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amzn-request-context",
        r#"{"identity":{"clientCert":{"subjectDN":"CN=device1,O=supervictor"}}}"#
            .parse()
            .unwrap(),
    );
    let subject = extract_client_subject(&headers);
    assert_eq!(subject.as_deref(), Some("CN=device1,O=supervictor"));
}

#[test]
fn extract_from_proxy_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ssl-client-subject-dn",
        "CN=device2,O=supervictor".parse().unwrap(),
    );
    let subject = extract_client_subject(&headers);
    assert_eq!(subject.as_deref(), Some("CN=device2,O=supervictor"));
}

#[test]
fn lwa_takes_priority_over_proxy() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amzn-request-context",
        r#"{"identity":{"clientCert":{"subjectDN":"CN=from-lwa"}}}"#
            .parse()
            .unwrap(),
    );
    headers.insert("x-ssl-client-subject-dn", "CN=from-proxy".parse().unwrap());
    let subject = extract_client_subject(&headers);
    assert_eq!(subject.as_deref(), Some("CN=from-lwa"));
}

#[test]
fn no_headers_returns_none() {
    let headers = HeaderMap::new();
    assert!(extract_client_subject(&headers).is_none());
}

#[test]
fn malformed_lwa_json_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("x-amzn-request-context", "{bad json}".parse().unwrap());
    assert!(extract_client_subject(&headers).is_none());
}

#[test]
fn lwa_missing_cert_falls_through() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amzn-request-context",
        r#"{"identity":{}}"#.parse().unwrap(),
    );
    assert!(extract_client_subject(&headers).is_none());
}

#[test]
fn lwa_missing_cert_falls_through_to_proxy() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amzn-request-context",
        r#"{"identity":{}}"#.parse().unwrap(),
    );
    headers.insert("x-ssl-client-subject-dn", "CN=fallback".parse().unwrap());
    let subject = extract_client_subject(&headers);
    assert_eq!(subject.as_deref(), Some("CN=fallback"));
}
