mod common;

use supervictor_endpoint::error::AppError;
use supervictor_endpoint::handlers;

#[test]
fn hello_without_subject() {
    let resp = handlers::handle_hello(None);
    assert_eq!(resp.message, "Hello from Supervictor!");
    assert!(resp.client_subject.is_none());
}

#[test]
fn hello_with_subject() {
    let resp = handlers::handle_hello(Some("CN=device1".into()));
    assert_eq!(resp.client_subject.as_deref(), Some("CN=device1"));
}

// ── Per-device power threshold ────────────────────────────────────────

fn registered(store: &dyn supervictor_endpoint::store::DeviceStore, id: &str) {
    let body = format!(r#"{{"device_id":"{id}","owner_id":"owner-1"}}"#);
    handlers::handle_register_device(Some(&body), store).unwrap();
}

#[test]
fn set_threshold_parses_every_body_shape() {
    let store = common::test_store();
    registered(store.as_ref(), "dev-1");

    let resp = handlers::handle_set_device_threshold(
        "dev-1",
        Some(r#"{"power_threshold_ma": 12000}"#),
        store.as_ref(),
    )
    .unwrap();
    assert_eq!(resp.power_threshold_ma, Some(12_000));
    assert_eq!(resp.device_id, "dev-1");

    for clear in [None, Some(""), Some("  \n"), Some("null"), Some("{}")] {
        handlers::handle_set_device_threshold(
            "dev-1",
            Some(r#"{"power_threshold_ma": 1}"#),
            store.as_ref(),
        )
        .unwrap();
        let resp = handlers::handle_set_device_threshold("dev-1", clear, store.as_ref())
            .unwrap_or_else(|e| panic!("{clear:?}: {e}"));
        assert_eq!(resp.power_threshold_ma, None, "{clear:?}");
    }
}

#[test]
fn set_threshold_rejects_bad_bodies_and_ids_without_writing() {
    use supervictor_endpoint::error::AppError;
    let store = common::test_store();
    registered(store.as_ref(), "dev-1");
    handlers::handle_set_device_threshold(
        "dev-1",
        Some(r#"{"power_threshold_ma": 42}"#),
        store.as_ref(),
    )
    .unwrap();

    for bad in [
        r#"{"power_threshold_ma": "42"}"#,
        r#"{"power_threshold_ma": 4.2}"#,
        r#"{"power_threshold_ma": 99999999999999999999}"#,
        r#"{"power_threshold_ma": [1]}"#,
        r#"{"threshold_ma": 1}"#,
        "not json",
        "[]",
        "true",
    ] {
        let err = handlers::handle_set_device_threshold("dev-1", Some(bad), store.as_ref())
            .expect_err(bad);
        assert!(
            matches!(err, AppError::InvalidPayload { .. }),
            "{bad}: {err:?}"
        );
    }
    assert_eq!(
        store
            .get_device("dev-1")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(42),
        "rejected bodies must not write"
    );

    // Identifier validation runs before the store is touched.
    for bad_id in ["", "dev\n1", &"x".repeat(65)] {
        let err = handlers::handle_set_device_threshold(
            bad_id,
            Some(r#"{"power_threshold_ma": 1}"#),
            store.as_ref(),
        )
        .expect_err(bad_id);
        assert!(
            matches!(err, AppError::InvalidPayload { .. }),
            "{bad_id:?}: {err:?}"
        );
    }
    let err = handlers::handle_set_device_threshold(
        "ghost",
        Some(r#"{"power_threshold_ma": 1}"#),
        store.as_ref(),
    )
    .unwrap_err();
    assert!(matches!(err, AppError::DeviceNotFound { .. }), "{err:?}");
}

#[test]
fn uplink_valid_payload() {
    let store = common::test_store();
    let body = r#"{"id": "dev-1", "current": 42}"#;
    let resp = handlers::handle_uplink(Some(body), None, Some(store.as_ref()), false).unwrap();
    assert_eq!(resp.device_id, "dev-1");
    assert_eq!(resp.current, 42);
    assert_eq!(resp.message, "Uplink received");
}

#[test]
fn uplink_empty_body() {
    let err = handlers::handle_uplink(None, None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("missing request body"), "got: {msg}");
}

#[test]
fn uplink_whitespace_body() {
    let err = handlers::handle_uplink(Some("  "), None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("missing request body"), "got: {msg}");
}

#[test]
fn uplink_invalid_json() {
    let err = handlers::handle_uplink(Some("{bad"), None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid payload"), "got: {msg}");
}

#[test]
fn uplink_wrong_schema() {
    let err = handlers::handle_uplink(Some(r#"{"foo": "bar"}"#), None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid payload"), "got: {msg}");
}

#[test]
fn uplink_with_client_subject() {
    let resp = handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 10}"#),
        Some("CN=test".into()),
        None,
        false,
    )
    .unwrap();
    assert_eq!(resp.client_subject.as_deref(), Some("CN=test"));
}

#[test]
fn uplink_stores_record() {
    let store = common::test_store();
    handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 99}"#),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();

    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    assert_eq!(uplinks.len(), 1);
    assert_eq!(uplinks[0].payload["current"], 99);
}

/// Every telemetry field the device sends is persisted; the id is not
/// duplicated into the payload (it's the record key), and fields the
/// device omitted don't appear as nulls.
#[test]
fn uplink_stores_full_telemetry_payload() {
    let store = common::test_store();
    let body = r#"{
        "id": "dev-1", "current": -5, "fw": "0.2.0",
        "seq": 17, "uptime_s": 86400, "interval_s": 300,
        "heap_free": 73728, "rssi_dbm": -67,
        "temp_c": 23.5, "humidity_pct": 41.25,
        "accel_g": [0.5, -0.25, 1.0],
        "gyro_dps": [1.5, 0.0, -1.5],
        "mag_ut": [22.5, -4.25, 40.0]
    }"#;
    handlers::handle_uplink(Some(body), None, Some(store.as_ref()), false).unwrap();

    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    assert_eq!(uplinks.len(), 1);
    let payload = uplinks[0].payload.as_object().unwrap();
    assert!(payload.get("id").is_none(), "id must not be duplicated");
    assert_eq!(payload["current"], -5);
    assert_eq!(payload["fw"], "0.2.0");
    assert_eq!(payload["seq"], 17);
    assert_eq!(payload["uptime_s"], 86400);
    assert_eq!(payload["interval_s"], 300);
    assert_eq!(payload["heap_free"], 73728);
    assert_eq!(payload["rssi_dbm"], -67);
    assert_eq!(payload["temp_c"], 23.5);
    assert_eq!(payload["humidity_pct"], 41.25);
    assert_eq!(payload["accel_g"], serde_json::json!([0.5, -0.25, 1.0]));
    assert_eq!(payload["gyro_dps"], serde_json::json!([1.5, 0.0, -1.5]));
    assert_eq!(payload["mag_ut"], serde_json::json!([22.5, -4.25, 40.0]));
}

/// A minimal (pre-telemetry) uplink stores a minimal payload — absent
/// Options never serialize as null into storage.
#[test]
fn uplink_minimal_payload_stays_minimal() {
    let store = common::test_store();
    handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 1}"#),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();

    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    let payload = uplinks[0].payload.as_object().unwrap();
    assert_eq!(
        payload.len(),
        1,
        "only 'current' expected, got: {payload:?}"
    );
    assert_eq!(payload["current"], 1);
}

#[test]
fn register_device_success() {
    let store = common::test_store();
    let body = r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#;
    let resp = handlers::handle_register_device(Some(body), store.as_ref()).unwrap();
    assert_eq!(resp.device_id, "dev-1");
    assert_eq!(resp.status, "active");
}

#[test]
fn register_device_duplicate() {
    let store = common::test_store();
    let body = r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#;
    handlers::handle_register_device(Some(body), store.as_ref()).unwrap();
    let err = handlers::handle_register_device(Some(body), store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("already exists"), "got: {msg}");
}

#[test]
fn register_device_empty_body() {
    let store = common::test_store();
    let err = handlers::handle_register_device(None, store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("missing request body"), "got: {msg}");
}

#[test]
fn register_device_invalid_json() {
    let store = common::test_store();
    let err = handlers::handle_register_device(Some("{bad"), store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid payload"), "got: {msg}");
}

#[test]
fn get_device_found() {
    let store = common::test_store();
    let body = r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#;
    handlers::handle_register_device(Some(body), store.as_ref()).unwrap();

    let resp = handlers::handle_get_device("dev-1", store.as_ref()).unwrap();
    assert_eq!(resp.device_id, "dev-1");
}

#[test]
fn get_device_not_found() {
    let store = common::test_store();
    let err = handlers::handle_get_device("nonexistent", store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("not found"), "got: {msg}");
}

#[test]
fn list_devices_empty() {
    let store = common::test_store();
    let resp = handlers::handle_list_devices(store.as_ref()).unwrap();
    assert!(resp.is_empty());
}

#[test]
fn list_devices_with_data() {
    let store = common::test_store();
    handlers::handle_register_device(
        Some(r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#),
        store.as_ref(),
    )
    .unwrap();
    handlers::handle_register_device(
        Some(r#"{"device_id": "dev-2", "owner_id": "owner-2"}"#),
        store.as_ref(),
    )
    .unwrap();

    let resp = handlers::handle_list_devices(store.as_ref()).unwrap();
    assert_eq!(resp.len(), 2);
}

#[test]
fn get_device_uplinks_empty() {
    let store = common::test_store();
    let resp = handlers::handle_get_device_uplinks("dev-1", store.as_ref(), 10).unwrap();
    assert!(resp.is_empty());
}

#[test]
fn full_roundtrip() {
    let store = common::test_store();

    // Register device
    handlers::handle_register_device(
        Some(r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#),
        store.as_ref(),
    )
    .unwrap();

    // Send uplink
    handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 42}"#),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();

    // Fetch uplinks
    let uplinks = handlers::handle_get_device_uplinks("dev-1", store.as_ref(), 10).unwrap();
    assert_eq!(uplinks.len(), 1);
    assert_eq!(uplinks[0].payload["current"], 42);
}

// ── levels bursts ─────────────────────────────────────────────────────

fn levels_body(levels: &str, ms: &str) -> String {
    let mut body = format!(r#"{{"id": "dev-1", "current": 1, "levels": {levels}"#);
    if !ms.is_empty() {
        body.push_str(&format!(r#", "levels_ms": {ms}"#));
    }
    body.push('}');
    body
}

/// A well-formed burst is stored byte for byte with its spacing, and the
/// record the route builds its live event from carries it too.
#[test]
fn uplink_stores_levels_burst() {
    use supervictor_common::levels::COUNT;
    let store = common::test_store();
    let flat: Vec<u8> = (0..(COUNT * 10) as u8).collect();
    let body = levels_body(&serde_json::to_string(&flat).unwrap(), "500");
    let ingested = handlers::ingest_uplink(Some(&body), None, Some(store.as_ref()), false).unwrap();
    assert_eq!(ingested.response.device_id, "dev-1");
    assert_eq!(ingested.record.device_id, "dev-1");
    assert_eq!(ingested.record.payload["levels_ms"], 500);
    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    assert_eq!(uplinks[0].payload["levels"], serde_json::json!(flat));
    assert_eq!(uplinks[0].payload["levels_ms"], 500);
    assert_eq!(uplinks[0].payload, ingested.record.payload);
    assert_eq!(uplinks[0].received_at, ingested.record.received_at);
}

/// Every malformed burst is a 422-class error and nothing is stored: a
/// device cannot park megabytes of bytes, a ragged array, or a burst with
/// no time base in a row.
#[test]
fn uplink_rejects_malformed_levels_bursts() {
    use supervictor_common::levels::COUNT;
    use supervictor_endpoint::validate::{MAX_LEVELS_MS, MAX_LEVELS_SAMPLES};
    let store = common::test_store();
    let one = serde_json::to_string(&vec![1u8; COUNT]).unwrap();
    let ragged = serde_json::to_string(&vec![1u8; COUNT + 1]).unwrap();
    let huge = serde_json::to_string(&vec![1u8; COUNT * (MAX_LEVELS_SAMPLES + 1)]).unwrap();
    let cases: Vec<(String, &str)> = vec![
        (levels_body(&one, ""), "levels_ms required"),
        (levels_body(&one, "0"), "levels_ms zero"),
        (
            levels_body(&one, &(MAX_LEVELS_MS + 1).to_string()),
            "levels_ms too long",
        ),
        (levels_body(&one, "-1"), "levels_ms negative"),
        (levels_body(&one, "1.5"), "levels_ms fractional"),
        (levels_body(&ragged, "500"), "ragged"),
        (levels_body("[1, 2, 3]", "500"), "short"),
        (levels_body(&huge, "500"), "too many samples"),
        (levels_body("[256, 1, 1, 1, 1]", "500"), "byte overflow"),
        (levels_body("[-1, 1, 1, 1, 1]", "500"), "negative byte"),
        (levels_body("\"AQIDBAU=\"", "500"), "string not array"),
        (levels_body("[]", "500"), "levels_ms without levels"),
        (levels_body("[[1,2,3,4,5]]", "500"), "nested"),
    ];
    for (body, what) in cases {
        let err =
            handlers::handle_uplink(Some(&body), None, Some(store.as_ref()), false).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("invalid payload"), "{what}: got {msg}");
        assert!(
            matches!(err, AppError::InvalidPayload { .. }),
            "{what}: {err:?}"
        );
    }
    assert!(
        store.get_uplinks("dev-1", 10).unwrap().is_empty(),
        "nothing stored"
    );
    // The largest legal burst is fine.
    let max = serde_json::to_string(&vec![255u8; COUNT * MAX_LEVELS_SAMPLES]).unwrap();
    handlers::handle_uplink(
        Some(&levels_body(&max, &MAX_LEVELS_MS.to_string())),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();
    // No burst at all is the common case and still fine.
    handlers::handle_uplink(
        Some(&levels_body("[]", "")),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();
}

// ── Per-device chart style ────────────────────────────────────────────

#[test]
fn set_style_parses_every_body_shape_and_normalises_colour() {
    use supervictor_common::models::{LineStyle, Marker};
    let store = common::test_store();
    registered(store.as_ref(), "dev-1");

    let resp = handlers::handle_set_device_style(
        "dev-1",
        Some(r##"{"color": "#ABCDEF", "line_style": "dashed", "marker": "circle"}"##),
        store.as_ref(),
    )
    .unwrap();
    assert_eq!(resp.color.as_deref(), Some("#abcdef"), "normalised");
    assert_eq!(resp.line_style, Some(LineStyle::Dashed));
    assert_eq!(resp.marker, Some(Marker::Circle));

    // A partial body replaces the whole style: the parts it omits clear.
    let resp =
        handlers::handle_set_device_style("dev-1", Some(r#"{"color": "s4"}"#), store.as_ref())
            .unwrap();
    assert_eq!(resp.color.as_deref(), Some("s4"));
    assert_eq!(resp.line_style, None);
    assert_eq!(resp.marker, None);

    for clear in [None, Some(""), Some("  \n"), Some("null"), Some("{}")] {
        handlers::handle_set_device_style("dev-1", Some(r#"{"color": "s1"}"#), store.as_ref())
            .unwrap();
        let resp = handlers::handle_set_device_style("dev-1", clear, store.as_ref())
            .unwrap_or_else(|e| panic!("{clear:?}: {e}"));
        assert_eq!(resp.color, None, "{clear:?}");
        assert!(
            store
                .get_device("dev-1")
                .unwrap()
                .unwrap()
                .style
                .is_default(),
            "{clear:?}"
        );
    }
}

#[test]
fn set_style_rejects_bad_bodies_without_writing() {
    use supervictor_endpoint::error::AppError;
    let store = common::test_store();
    registered(store.as_ref(), "dev-1");
    handlers::handle_set_device_style("dev-1", Some(r#"{"color": "s6"}"#), store.as_ref()).unwrap();
    for bad in [
        "[]",
        "42",
        "\"s1\"",
        r#"{"colour": "s1"}"#,
        r#"{"color": "s8"}"#,
        r#"{"color": "red"}"#,
        r##"{"color": "#abc"}"##,
        r#"{"color": 3}"#,
        r#"{"line_style": "wavy"}"#,
        r#"{"marker": "star"}"#,
        r#"{"marker": null, "extra": true}"#,
        "{",
    ] {
        let err =
            handlers::handle_set_device_style("dev-1", Some(bad), store.as_ref()).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidPayload { .. }),
            "{bad}: {err:?}"
        );
        assert_eq!(
            store
                .get_device("dev-1")
                .unwrap()
                .unwrap()
                .style
                .color
                .as_deref(),
            Some("s6"),
            "{bad} must not write"
        );
    }
    let err = handlers::handle_set_device_style("ghost", Some("{}"), store.as_ref()).unwrap_err();
    assert!(matches!(err, AppError::DeviceNotFound { .. }), "{err:?}");
    let err = handlers::handle_set_device_style("", Some("{}"), store.as_ref()).unwrap_err();
    assert!(matches!(err, AppError::InvalidPayload { .. }), "{err:?}");
}

/// Registration without a colour stores the first palette slot the fleet
/// is not wearing; with one, it is validated and kept.
#[test]
fn register_assigns_an_unworn_palette_colour() {
    use supervictor_endpoint::error::AppError;
    let store = common::test_store();
    let first = handlers::handle_register_device(
        Some(r#"{"device_id":"one","owner_id":"o"}"#),
        store.as_ref(),
    )
    .unwrap();
    assert_eq!(first.color.as_deref(), Some("s0"));
    let picked = handlers::handle_register_device(
        Some(r#"{"device_id":"two","owner_id":"o","color":"s1","line_style":"dotted"}"#),
        store.as_ref(),
    )
    .unwrap();
    assert_eq!(picked.color.as_deref(), Some("s1"));
    assert_eq!(
        picked.line_style,
        Some(supervictor_common::models::LineStyle::Dotted)
    );
    let third = handlers::handle_register_device(
        Some(r#"{"device_id":"three","owner_id":"o"}"#),
        store.as_ref(),
    )
    .unwrap();
    assert_eq!(third.color.as_deref(), Some("s2"), "s0 and s1 are worn");
    let err = handlers::handle_register_device(
        Some(r#"{"device_id":"four","owner_id":"o","color":"orange"}"#),
        store.as_ref(),
    )
    .unwrap_err();
    assert!(matches!(err, AppError::InvalidPayload { .. }), "{err:?}");
    assert!(
        store.get_device("four").unwrap().is_none(),
        "rejected, not stored"
    );
}
