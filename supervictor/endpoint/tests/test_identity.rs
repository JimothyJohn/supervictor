//! Uplink identity: a registered device's `subject_dn` binds it to one mTLS
//! certificate. Regression suite for the spoofing hole where `handle_uplink`
//! trusted the payload `id` and never looked at the client cert — every
//! `*_rejected` test here fails against that code.

mod common;

use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum_test::TestServer;
use supervictor_endpoint::error::AppError;
use supervictor_endpoint::handlers::{authorize_uplink, handle_register_device, handle_uplink};
use supervictor_endpoint::models::DeviceRecord;
use supervictor_endpoint::routes::{router, router_with, RouterOptions};
use supervictor_endpoint::store::DeviceStore;

const BOUND_DN: &str = "CN=dev-1,OU=devices,O=supervictor";
const OTHER_DN: &str = "CN=dev-2,OU=devices,O=supervictor";
const SUBJECT_HEADER: &str = "x-ssl-client-subject-dn";

fn record(id: &str, status: &str, subject_dn: Option<&str>) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "owner-1".into(),
        subject_dn: subject_dn.map(str::to_string),
        status: status.into(),
        created_at: "2026-07-01T00:00:00+00:00".into(),
        power_threshold_ma: None,
        style: Default::default(),
    }
}

fn body(id: &str) -> String {
    format!(r#"{{"id":"{id}","current":1}}"#)
}

fn uplink(
    store: &dyn DeviceStore,
    id: &str,
    subject: Option<&str>,
    require_registration: bool,
) -> Result<(), AppError> {
    handle_uplink(
        Some(&body(id)),
        subject.map(str::to_string),
        Some(store),
        require_registration,
    )
    .map(|_| ())
}

// ── authorize_uplink: the pure policy, exhaustively ──────────────────

#[test]
fn policy_unregistered_device_open_ingest_vs_required() {
    assert!(authorize_uplink("ghost", None, None, false).is_ok());
    assert!(authorize_uplink("ghost", None, Some(BOUND_DN), false).is_ok());
    assert!(matches!(
        authorize_uplink("ghost", None, Some(BOUND_DN), true),
        Err(AppError::DeviceNotRegistered)
    ));
}

#[test]
fn policy_bound_device_requires_matching_subject_regardless_of_flag() {
    let bound = record("dev-1", "active", Some(BOUND_DN));
    for required in [false, true] {
        assert!(authorize_uplink("dev-1", Some(&bound), Some(BOUND_DN), required).is_ok());
        assert!(
            matches!(
                authorize_uplink("dev-1", Some(&bound), Some(OTHER_DN), required),
                Err(AppError::SubjectMismatch { ref device_id }) if device_id == "dev-1"
            ),
            "other device's cert must be rejected (require_registration={required})"
        );
        assert!(
            matches!(
                authorize_uplink("dev-1", Some(&bound), None, required),
                Err(AppError::SubjectMismatch { .. })
            ),
            "no cert at all must be rejected for a bound device (require_registration={required})"
        );
    }
}

#[test]
fn policy_unbound_device_accepted_only_without_the_flag() {
    let unbound = record("dev-1", "active", None);
    // Open ingest: an unbound registration is not a binding, nothing to
    // compare against.
    assert!(authorize_uplink("dev-1", Some(&unbound), None, false).is_ok());
    assert!(authorize_uplink("dev-1", Some(&unbound), Some(OTHER_DN), false).is_ok());
    // Strict: registered-but-unbound is not fully registered.
    for subject in [None, Some(OTHER_DN)] {
        assert!(
            matches!(
                authorize_uplink("dev-1", Some(&unbound), subject, true),
                Err(AppError::DeviceNotRegistered)
            ),
            "unbound device must be rejected under REQUIRE_REGISTRATION (subject={subject:?})"
        );
    }
}

#[test]
fn policy_inactive_device() {
    let inactive = record("dev-1", "inactive", Some(BOUND_DN));
    // Open ingest: status is not consulted, but binding still is.
    assert!(authorize_uplink("dev-1", Some(&inactive), Some(BOUND_DN), false).is_ok());
    assert!(matches!(
        authorize_uplink("dev-1", Some(&inactive), Some(OTHER_DN), false),
        Err(AppError::SubjectMismatch { .. })
    ));
    // Required: inactive is rejected even with the right cert.
    assert!(matches!(
        authorize_uplink("dev-1", Some(&inactive), Some(BOUND_DN), true),
        Err(AppError::DeviceNotRegistered)
    ));
}

#[test]
fn policy_blank_bound_dn_fails_closed() {
    let blank = record("dev-1", "active", Some(""));
    assert!(matches!(
        authorize_uplink("dev-1", Some(&blank), Some(""), false),
        Err(AppError::SubjectMismatch { .. })
    ));
    assert!(matches!(
        authorize_uplink("dev-1", Some(&blank), None, false),
        Err(AppError::SubjectMismatch { .. })
    ));
}

// ── handle_uplink against a real store ───────────────────────────────

#[test]
fn spoofed_uplink_is_rejected_and_not_stored() {
    let store = common::test_store();
    store
        .put_device(record("dev-1", "active", Some(BOUND_DN)))
        .unwrap();
    store
        .put_device(record("dev-2", "active", Some(OTHER_DN)))
        .unwrap();

    // dev-2's certificate, dev-1's payload.
    let err = uplink(store.as_ref(), "dev-1", Some(OTHER_DN), false).unwrap_err();
    assert!(
        matches!(err, AppError::SubjectMismatch { .. }),
        "got {err:?}"
    );
    assert!(
        store.get_uplinks("dev-1", 10).unwrap().is_empty(),
        "a rejected uplink must leave no row"
    );

    // The real dev-1 still gets through.
    uplink(store.as_ref(), "dev-1", Some(BOUND_DN), false).unwrap();
    assert_eq!(store.get_uplinks("dev-1", 10).unwrap().len(), 1);
}

#[test]
fn bound_device_without_cert_is_rejected() {
    // The execute-api path skips API Gateway mTLS: no clientCert, no subject.
    let store = common::test_store();
    store
        .put_device(record("dev-1", "active", Some(BOUND_DN)))
        .unwrap();
    let err = uplink(store.as_ref(), "dev-1", None, false).unwrap_err();
    assert!(
        matches!(err, AppError::SubjectMismatch { .. }),
        "got {err:?}"
    );
}

#[test]
fn proxy_formatting_of_the_same_subject_is_accepted() {
    let store = common::test_store();
    store
        .put_device(record("dev-1", "active", Some(BOUND_DN)))
        .unwrap();
    uplink(
        store.as_ref(),
        "dev-1",
        Some("O=supervictor, OU=devices, CN=dev-1"),
        false,
    )
    .unwrap();
}

#[test]
fn registration_flow_binds_the_subject() {
    // Register exactly as `Quickstart onboard` does, then uplink as the
    // device, then try to spoof it.
    let store = common::test_store();
    handle_register_device(
        Some(&format!(
            r#"{{"device_id":"dev-1","owner_id":"o1","subject_dn":"{BOUND_DN}"}}"#
        )),
        store.as_ref(),
    )
    .unwrap();
    uplink(store.as_ref(), "dev-1", Some(BOUND_DN), true).unwrap();
    assert!(uplink(store.as_ref(), "dev-1", Some(OTHER_DN), true).is_err());
}

#[test]
fn require_registration_rejects_unknown_unbound_and_inactive() {
    let store = common::test_store();
    assert!(matches!(
        uplink(store.as_ref(), "ghost", Some(BOUND_DN), true),
        Err(AppError::DeviceNotRegistered)
    ));
    store.put_device(record("dev-1", "active", None)).unwrap();
    assert!(
        matches!(
            uplink(store.as_ref(), "dev-1", Some(BOUND_DN), true),
            Err(AppError::DeviceNotRegistered)
        ),
        "active but unbound: rejected under the flag"
    );
    uplink(store.as_ref(), "dev-1", None, false).unwrap();
    store.set_device_status("dev-1", "inactive").unwrap();
    assert!(matches!(
        uplink(store.as_ref(), "dev-1", None, true),
        Err(AppError::DeviceNotRegistered)
    ));
    assert_eq!(store.get_uplinks("dev-1", 10).unwrap().len(), 1);
}

// ── Payload validation ───────────────────────────────────────────────

fn assert_invalid(result: Result<(), AppError>, field: &str) {
    match result {
        Err(AppError::InvalidPayload { detail, structured }) => {
            assert!(detail.starts_with(field), "detail {detail:?}");
            assert_eq!(structured.unwrap()["field"], field);
        }
        other => panic!("expected InvalidPayload on {field}, got {other:?}"),
    }
}

#[test]
fn uplink_id_bounds() {
    let store = common::test_store();
    let max = "x".repeat(64);
    uplink(store.as_ref(), &max, None, false).unwrap();
    assert_invalid(uplink(store.as_ref(), &"x".repeat(65), None, false), "id");
    assert_invalid(uplink(store.as_ref(), "", None, false), "id");
    assert_invalid(uplink(store.as_ref(), "   ", None, false), "id");
    // Multibyte: the firmware limit is bytes, not chars.
    assert_invalid(uplink(store.as_ref(), &"é".repeat(33), None, false), "id");
    uplink(store.as_ref(), &"é".repeat(32), None, false).unwrap();
}

#[test]
fn uplink_id_control_characters_rejected() {
    let store = common::test_store();
    for bad in ["dev\n1", "dev\r1", "dev\t1", "dev\u{1b}[31m1", "dev\u{0}1"] {
        let err = handle_uplink(
            Some(&serde_json::json!({"id": bad, "current": 1}).to_string()),
            None,
            Some(store.as_ref()),
            false,
        )
        .map(|_| ());
        assert_invalid(err, "id");
    }
    assert!(store.list_devices().unwrap().is_empty());
}

#[test]
fn registration_field_bounds() {
    let store = common::test_store();
    let reg = |json: serde_json::Value| {
        handle_register_device(Some(&json.to_string()), store.as_ref()).map(|_| ())
    };
    assert_invalid(
        reg(serde_json::json!({"device_id": "", "owner_id": "o"})),
        "device_id",
    );
    assert_invalid(
        reg(serde_json::json!({"device_id": "x".repeat(65), "owner_id": "o"})),
        "device_id",
    );
    assert_invalid(
        reg(serde_json::json!({"device_id": "d", "owner_id": ""})),
        "owner_id",
    );
    assert_invalid(
        reg(serde_json::json!({"device_id": "d", "owner_id": "o\u{7f}"})),
        "owner_id",
    );
    assert_invalid(
        reg(serde_json::json!({"device_id": "d", "owner_id": "o", "subject_dn": ""})),
        "subject_dn",
    );
    assert_invalid(
        reg(serde_json::json!({"device_id": "d", "owner_id": "o", "subject_dn": "CN=a\nOU=admin"})),
        "subject_dn",
    );
    assert_invalid(
        reg(serde_json::json!({"device_id": "d", "owner_id": "o", "subject_dn": "x".repeat(1025)})),
        "subject_dn",
    );
    assert!(
        store.list_devices().unwrap().is_empty(),
        "nothing partial persisted"
    );
    reg(serde_json::json!({"device_id": "d", "owner_id": "o", "subject_dn": "CN=d"})).unwrap();
}

// ── Over HTTP ────────────────────────────────────────────────────────

fn header(value: &str) -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static(SUBJECT_HEADER),
        HeaderValue::from_str(value).unwrap(),
    )
}

#[tokio::test]
async fn http_spoof_is_403_and_leaks_nothing() {
    let store = common::test_store();
    store
        .put_device(record("dev-1", "active", Some(BOUND_DN)))
        .unwrap();
    let server = TestServer::new(router(store.clone()));

    let (name, value) = header(OTHER_DN);
    let resp = server
        .post("/")
        .add_header(name, value)
        .text(body("dev-1"))
        .await;
    resp.assert_status(StatusCode::FORBIDDEN);
    let text = resp.text();
    assert!(
        !text.contains("dev-1,OU") && !text.contains("supervictor"),
        "expected DN must not be echoed: {text}"
    );
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["error"], "Client certificate does not match device");

    // No cert header at all.
    let resp = server.post("/").text(body("dev-1")).await;
    resp.assert_status(StatusCode::FORBIDDEN);

    // Right cert.
    let (name, value) = header(BOUND_DN);
    let resp = server
        .post("/")
        .add_header(name, value)
        .text(body("dev-1"))
        .await;
    resp.assert_status_ok();
    assert_eq!(store.get_uplinks("dev-1", 10).unwrap().len(), 1);
}

#[tokio::test]
async fn http_lambda_request_context_subject_is_bound_too() {
    let store = common::test_store();
    store
        .put_device(record("dev-1", "active", Some(BOUND_DN)))
        .unwrap();
    let server = TestServer::new(router(store));
    let ctx = |dn: &str| {
        (
            HeaderName::from_static("x-amzn-request-context"),
            HeaderValue::from_str(&format!(
                r#"{{"identity":{{"clientCert":{{"subjectDN":"{dn}"}}}}}}"#
            ))
            .unwrap(),
        )
    };
    let (n, v) = ctx(OTHER_DN);
    server
        .post("/")
        .add_header(n, v)
        .text(body("dev-1"))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    let (n, v) = ctx(BOUND_DN);
    server
        .post("/")
        .add_header(n, v)
        .text(body("dev-1"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn http_require_registration_option() {
    let store = common::test_store();
    store.put_device(record("dev-1", "active", None)).unwrap();
    store
        .put_device(record("dev-2", "active", Some(BOUND_DN)))
        .unwrap();
    let server = TestServer::new(router_with(
        store,
        RouterOptions {
            require_registration: true,
            ..RouterOptions::default()
        },
    ));
    server
        .post("/")
        .text(body("ghost"))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // Registered but unbound: not enough under the flag.
    server
        .post("/")
        .text(body("dev-1"))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // Registered, bound, matching cert: through.
    let (name, value) = header(BOUND_DN);
    server
        .post("/")
        .add_header(name, value)
        .text(body("dev-2"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn http_invalid_id_is_422() {
    let server = TestServer::new(router(common::test_store()));
    let resp = server.post("/").text(body(&"x".repeat(65))).await;
    resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let json: serde_json::Value = resp.json();
    assert_eq!(json["detail"]["field"], "id");
}
