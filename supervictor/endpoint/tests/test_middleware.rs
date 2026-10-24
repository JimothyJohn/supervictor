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

/// Regression (2026-10-24): behind the adapter, a request with no client
/// certificate must have NO identity. Falling through to the proxy header let
/// any caller pick a device subject on dev and prod (raw execute-api URL, or
/// the mTLS domain with someone else's valid cert).
#[test]
fn lwa_present_without_cert_ignores_forged_proxy_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amzn-request-context",
        r#"{"identity":{"sourceIp":"1.2.3.4"}}"#.parse().unwrap(),
    );
    headers.insert(
        "x-ssl-client-subject-dn",
        "OU=devices,O=Supervictor,CN=c3-wifi-01".parse().unwrap(),
    );
    assert!(extract_client_subject(&headers).is_none());
}

/// Same when the adapter header is present but unparseable: still no fallback.
#[test]
fn lwa_malformed_ignores_forged_proxy_header() {
    let mut headers = HeaderMap::new();
    headers.insert("x-amzn-request-context", "{bad json}".parse().unwrap());
    headers.insert("x-ssl-client-subject-dn", "CN=forged".parse().unwrap());
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
