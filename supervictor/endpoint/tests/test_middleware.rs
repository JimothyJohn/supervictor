use axum::http::HeaderMap;
use supervictor_endpoint::middleware::extract_client_subject;

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
fn no_headers_returns_none() {
    let headers = HeaderMap::new();
    assert!(extract_client_subject(&headers).is_none());
}

/// The retired Lambda Web Adapter header is just another unknown header
/// now: it neither grants an identity nor shadows the proxy convention.
#[test]
fn adapter_request_context_is_ignored() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amzn-request-context",
        r#"{"identity":{"clientCert":{"subjectDN":"CN=from-adapter"}}}"#
            .parse()
            .unwrap(),
    );
    assert!(extract_client_subject(&headers).is_none());
    headers.insert("x-ssl-client-subject-dn", "CN=from-proxy".parse().unwrap());
    assert_eq!(
        extract_client_subject(&headers).as_deref(),
        Some("CN=from-proxy")
    );
}

/// A non-UTF-8 header value is no identity, not a panic.
#[test]
fn undecodable_proxy_header_is_none() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ssl-client-subject-dn",
        axum::http::HeaderValue::from_bytes(b"CN=\xff\xfe").unwrap(),
    );
    assert!(extract_client_subject(&headers).is_none());
}

// ── dn_matches ────────────────────────────────────────────────────────

mod dn_matches {
    use supervictor_endpoint::middleware::dn_matches;

    #[test]
    fn identical_strings_match() {
        assert!(dn_matches(
            "CN=dev-1,OU=devices,O=supervictor",
            "CN=dev-1,OU=devices,O=supervictor"
        ));
    }

    #[test]
    fn proxy_formatting_differences_are_tolerated() {
        // Caddy / openssl spacing and component order vary for one cert.
        let rfc2253 = "CN=dev-1,OU=devices,O=supervictor";
        assert!(dn_matches(rfc2253, "CN=dev-1, OU=devices, O=supervictor"));
        assert!(dn_matches(rfc2253, "O=supervictor,OU=devices,CN=dev-1"));
        assert!(dn_matches(
            rfc2253,
            " cn=dev-1 , ou=devices , o=supervictor "
        ));
    }

    #[test]
    fn values_are_case_sensitive() {
        // Only attribute types fold; `CN=Dev-1` is a different subject.
        assert!(!dn_matches("CN=dev-1", "CN=Dev-1"));
    }

    #[test]
    fn prefix_and_superset_subjects_do_not_match() {
        assert!(!dn_matches("CN=dev-1", "CN=dev-10"));
        assert!(!dn_matches("CN=dev-1", "CN=dev-1,OU=admin"));
        assert!(!dn_matches("CN=dev-1,OU=admin", "CN=dev-1"));
        assert!(!dn_matches("CN=dev-1,OU=devices", "CN=dev-1,OU=admin"));
    }

    #[test]
    fn empty_never_matches_even_empty() {
        assert!(!dn_matches("", ""));
        assert!(!dn_matches("", "CN=dev-1"));
        assert!(!dn_matches("CN=dev-1", ""));
        assert!(!dn_matches(" , , ", ","));
    }

    #[test]
    fn duplicate_components_are_significant() {
        assert!(!dn_matches("CN=a,CN=a", "CN=a"));
    }

    #[test]
    fn is_symmetric_and_reflexive_for_real_subjects() {
        for dn in [
            "CN=dev-1",
            "CN=dev-1,OU=devices,O=supervictor",
            "CN=ops,OU=admin,O=supervictor,C=US",
        ] {
            assert!(dn_matches(dn, dn), "{dn} must match itself");
            let reversed: Vec<&str> = dn.split(',').rev().collect();
            let reversed = reversed.join(",");
            assert!(dn_matches(dn, &reversed));
            assert!(dn_matches(&reversed, dn));
        }
    }
}
