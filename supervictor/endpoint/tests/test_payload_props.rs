//! Property tests for payload validation and the identity helpers.
//!
//! The example-based suites pin specific behaviours; these pin the
//! invariants for *any* input: no panic on arbitrary bytes, the validator
//! is complete in both directions (accepts iff the invariants hold), and
//! an accepted uplink is stored losslessly.

mod common;

use std::collections::BTreeSet;

use axum::http::{HeaderMap, HeaderValue};
use proptest::prelude::*;
use supervictor_endpoint::error::AppError;
use supervictor_endpoint::handlers::{handle_register_device, handle_uplink};
use supervictor_endpoint::middleware::{
    dn_matches, extract_client_subject, is_admin_subject, sanitize_for_log,
};
use supervictor_endpoint::models::UplinkMessage;
use supervictor_endpoint::validate::{check_text, MAX_DEVICE_ID_LEN};

// ── Strategies ───────────────────────────────────────────────────────

/// Identifiers the validator must accept: 1..=64 bytes, printable ASCII.
fn valid_id() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_.:-]{1,64}"
}

fn finite_f32() -> impl Strategy<Value = f32> {
    -1.0e6f32..1.0e6f32
}

fn triple() -> impl Strategy<Value = [f32; 3]> {
    [finite_f32(), finite_f32(), finite_f32()]
}

prop_compose! {
    fn uplink_message()(
        id in valid_id(),
        current in any::<i32>(),
        fw in proptest::option::of("[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}"),
        health in (
            proptest::option::of(any::<u32>()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(any::<i8>()),
        ),
        env in (
            proptest::option::of(finite_f32()),
            proptest::option::of(finite_f32()),
            proptest::option::of(triple()),
            proptest::option::of(triple()),
            proptest::option::of(triple()),
        ),
    ) -> UplinkMessage {
        let (seq, uptime_s, interval_s, heap_free, rssi_dbm) = health;
        let (temp_c, humidity_pct, accel_g, gyro_dps, mag_ut) = env;
        UplinkMessage {
            id, current, fw, seq, uptime_s, interval_s, heap_free, rssi_dbm,
            temp_c, humidity_pct, accel_g, gyro_dps, mag_ut,
        }
    }
}

/// A DN as components, so renderings can be varied independently of
/// identity. Values avoid `,` and `=`, which the component split treats
/// as structure.
fn dn_components() -> impl Strategy<Value = Vec<(String, String)>> {
    proptest::collection::vec(("(CN|OU|O|L|ST|C|DC)", "[A-Za-z0-9 ._-]{1,16}"), 1..6)
}

fn render(components: &[(String, String)], spaced: bool, lowercase_keys: bool) -> String {
    components
        .iter()
        .map(|(k, v)| {
            let k = if lowercase_keys {
                k.to_ascii_lowercase()
            } else {
                k.clone()
            };
            if spaced {
                format!(" {k} = {v} ")
            } else {
                format!("{k}={v}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn id_is_valid(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= MAX_DEVICE_ID_LEN && !id.chars().any(char::is_control)
}

// ── Uplink ingest ────────────────────────────────────────────────────

proptest! {
    /// Any well-formed telemetry message is accepted and stored losslessly:
    /// payload + id round-trips to the original, and absent fields stay
    /// absent (no nulls accrete in storage).
    #[test]
    fn well_formed_uplink_round_trips_through_storage(msg in uplink_message()) {
        let store = common::test_store();
        let body = serde_json::to_string(&msg).unwrap();
        let resp = handle_uplink(Some(&body), None, Some(store.as_ref()), false)
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        prop_assert_eq!(&resp.device_id, &msg.id);
        prop_assert_eq!(resp.current, msg.current);

        let rows = store.get_uplinks(&msg.id, 10).unwrap();
        prop_assert_eq!(rows.len(), 1);
        let mut payload = rows[0].payload.clone();
        let obj = payload.as_object_mut().unwrap();
        prop_assert!(obj.get("id").is_none(), "id duplicated into payload");

        let some_fields = 1 // current
            + msg.fw.is_some() as usize
            + msg.seq.is_some() as usize
            + msg.uptime_s.is_some() as usize
            + msg.interval_s.is_some() as usize
            + msg.heap_free.is_some() as usize
            + msg.rssi_dbm.is_some() as usize
            + msg.temp_c.is_some() as usize
            + msg.humidity_pct.is_some() as usize
            + msg.accel_g.is_some() as usize
            + msg.gyro_dps.is_some() as usize
            + msg.mag_ut.is_some() as usize;
        prop_assert_eq!(obj.len(), some_fields, "payload: {:?}", obj);

        obj.insert("id".into(), serde_json::Value::String(msg.id.clone()));
        let recovered: UplinkMessage = serde_json::from_value(payload).unwrap();
        prop_assert_eq!(recovered, msg);
    }

    /// Arbitrary bytes never panic and never reach the store: the only
    /// outcomes are a client error or (if it happened to parse) an
    /// accepted uplink whose id satisfies the invariants.
    #[test]
    fn arbitrary_bytes_never_panic_or_leak_into_storage(
        bytes in proptest::collection::vec(any::<u8>(), 0..512)
    ) {
        let store = common::test_store();
        let text = String::from_utf8_lossy(&bytes);
        match handle_uplink(Some(&text), None, Some(store.as_ref()), false) {
            Ok(resp) => {
                prop_assert!(id_is_valid(&resp.device_id));
                prop_assert_eq!(store.get_uplinks(&resp.device_id, 10).unwrap().len(), 1);
            }
            Err(AppError::MissingBody) | Err(AppError::InvalidPayload { .. }) => {
                prop_assert!(store.latest_uplinks().unwrap().is_empty());
            }
            Err(other) => prop_assert!(false, "unexpected error class: {other:?}"),
        }
    }

    /// For a well-formed envelope, acceptance is exactly the id invariant —
    /// nothing else about the id (unicode, quoting, length in chars) matters.
    #[test]
    fn uplink_id_accepted_iff_invariants_hold(
        id in any::<String>(),
        current in any::<i32>(),
    ) {
        let store = common::test_store();
        let body = serde_json::json!({ "id": id, "current": current }).to_string();
        let result = handle_uplink(Some(&body), None, Some(store.as_ref()), false);
        if id_is_valid(&id) {
            let resp = result.map_err(|e| TestCaseError::fail(format!("{e}")))?;
            prop_assert_eq!(resp.device_id, id);
        } else {
            prop_assert!(
                matches!(result, Err(AppError::InvalidPayload { .. })),
                "id {id:?} should have been rejected, got {result:?}"
            );
            prop_assert!(store.latest_uplinks().unwrap().is_empty());
        }
    }

    /// Short ids (the firmware's own range) are also accepted when they
    /// are multibyte but still within 64 bytes.
    #[test]
    fn unicode_ids_within_64_bytes_are_accepted(id in "\\PC{1,16}") {
        prop_assume!(id_is_valid(&id));
        let store = common::test_store();
        let body = serde_json::json!({ "id": id, "current": 0 }).to_string();
        handle_uplink(Some(&body), None, Some(store.as_ref()), false)
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
    }
}

// ── Registration ─────────────────────────────────────────────────────

proptest! {
    #[test]
    fn registration_accepted_iff_every_field_validates(
        device_id in any::<String>(),
        owner_id in any::<String>(),
        subject_dn in proptest::option::of(any::<String>()),
    ) {
        let store = common::test_store();
        let body = serde_json::json!({
            "device_id": device_id,
            "owner_id": owner_id,
            "subject_dn": subject_dn,
        })
        .to_string();
        let expected_ok = check_text("device_id", &device_id, 64).is_ok()
            && check_text("owner_id", &owner_id, 128).is_ok()
            && subject_dn
                .as_deref()
                .is_none_or(|dn| check_text("subject_dn", dn, 1024).is_ok());
        let result = handle_register_device(Some(&body), store.as_ref());
        prop_assert_eq!(result.is_ok(), expected_ok, "{:?}", result);
        if let Ok(resp) = result {
            prop_assert_eq!(resp.device_id, device_id);
            prop_assert_eq!(resp.owner_id, owner_id);
            prop_assert_eq!(resp.subject_dn, subject_dn);
            prop_assert_eq!(store.list_devices().unwrap().len(), 1);
        } else {
            prop_assert!(store.list_devices().unwrap().is_empty());
        }
    }
}

// ── validate::check_text ─────────────────────────────────────────────

proptest! {
    #[test]
    fn check_text_is_complete(value in any::<String>(), max_len in 1usize..256) {
        let ok = check_text("f", &value, max_len).is_ok();
        let invariant = !value.trim().is_empty()
            && value.len() <= max_len
            && !value.chars().any(char::is_control);
        prop_assert_eq!(ok, invariant, "value {:?} max_len {}", value, max_len);
    }

    #[test]
    fn check_text_error_names_the_field(field in "[a-z_]{1,16}", value in "\\PC{0,8}") {
        if let Err(AppError::InvalidPayload { detail, structured }) =
            check_text(&field, &value, 4)
        {
            prop_assert!(detail.starts_with(&field));
            let structured = structured.unwrap();
            prop_assert_eq!(structured["field"].as_str().unwrap(), field.as_str());
        }
    }
}

// ── dn_matches ───────────────────────────────────────────────────────

proptest! {
    #[test]
    fn dn_matches_is_symmetric(a in any::<String>(), b in any::<String>()) {
        prop_assert_eq!(dn_matches(&a, &b), dn_matches(&b, &a));
    }

    /// Spacing, attribute-type case and component order are rendering
    /// details of one subject; every rendering matches every other.
    #[test]
    fn dn_matches_is_invariant_under_rendering(
        components in dn_components(),
        shuffle in any::<prop::sample::Index>(),
        spaced in any::<bool>(),
        lower in any::<bool>(),
    ) {
        let canonical = render(&components, false, false);
        prop_assert!(dn_matches(&canonical, &canonical));
        let mut rotated = components.clone();
        rotated.rotate_left(shuffle.index(components.len()));
        prop_assert!(dn_matches(&canonical, &render(&rotated, spaced, lower)));
    }

    /// Changing any single value yields a different subject.
    #[test]
    fn dn_matches_detects_a_changed_value(
        components in dn_components(),
        which in any::<prop::sample::Index>(),
        suffix in "[A-Za-z0-9]{1,4}",
    ) {
        let i = which.index(components.len());
        let mut changed = components.clone();
        changed[i].1 = format!("{}{suffix}", changed[i].1);
        // Guard against the (rare) case where another component already
        // carries the mutated pair, which would make the multisets equal
        // only if a duplicate existed — it can't, we appended to one entry.
        let before: BTreeSet<_> = components.iter().collect();
        let after: BTreeSet<_> = changed.iter().collect();
        prop_assume!(before != after);
        prop_assert!(!dn_matches(&render(&components, false, false), &render(&changed, false, false)));
    }

    /// Adding or dropping a component yields a different subject — a
    /// superset DN must not pass as the registered one.
    #[test]
    fn dn_matches_detects_added_component(
        components in dn_components(),
        extra_key in "(CN|OU|O)",
        extra_val in "[A-Za-z0-9]{1,8}",
    ) {
        let mut more = components.clone();
        more.push((extra_key, extra_val));
        prop_assert!(!dn_matches(&render(&components, false, false), &render(&more, false, false)));
    }
}

// ── is_admin_subject / sanitize_for_log / extract_client_subject ────

proptest! {
    #[test]
    fn is_admin_subject_never_panics_and_is_component_exact(s in any::<String>()) {
        let admin = is_admin_subject(&s);
        let expected = s.split(',').any(|c| {
            let mut kv = c.trim().splitn(2, '=');
            let k = kv.next().unwrap_or("").trim();
            let v = kv.next().unwrap_or("").trim();
            k.eq_ignore_ascii_case("OU") && v.eq_ignore_ascii_case("admin")
        });
        prop_assert_eq!(admin, expected);
    }

    /// A value smuggled into another attribute never grants admin.
    #[test]
    fn admin_marker_inside_a_value_is_not_admin(v in "[^,]{0,32}") {
        // (prop_assert! stringifies its condition into a format string, so
        // the braces have to live outside it.)
        let plain = format!("CN={v}");
        let with_ou = format!("CN={v},OU=devices");
        prop_assert!(!is_admin_subject(&plain), "{}", plain);
        prop_assert!(!is_admin_subject(&with_ou), "{}", with_ou);
    }

    #[test]
    fn sanitize_for_log_strips_exactly_the_control_chars(s in any::<String>()) {
        let out = sanitize_for_log(&s);
        prop_assert!(!out.chars().any(char::is_control));
        let expected: String = s.chars().filter(|c| !c.is_control()).collect();
        prop_assert_eq!(&out, &expected);
        prop_assert_eq!(sanitize_for_log(&out), out);
    }

    #[test]
    fn extract_client_subject_never_panics_on_arbitrary_headers(
        ctx in proptest::option::of(proptest::collection::vec(any::<u8>(), 0..256)),
        dn in proptest::option::of(proptest::collection::vec(any::<u8>(), 0..128)),
    ) {
        let mut headers = HeaderMap::new();
        if let Some(v) = ctx.as_deref().and_then(|b| HeaderValue::from_bytes(b).ok()) {
            headers.insert("x-amzn-request-context", v);
        }
        if let Some(v) = dn.as_deref().and_then(|b| HeaderValue::from_bytes(b).ok()) {
            headers.insert("x-ssl-client-subject-dn", v);
        }
        let subject = extract_client_subject(&headers);
        if headers.is_empty() {
            prop_assert!(subject.is_none());
        }
        if let Some(s) = subject {
            // Whatever came out must be a substring of one of the headers.
            let from_ctx = headers
                .get("x-amzn-request-context")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|c| c.contains(&s) || serde_json::from_str::<serde_json::Value>(c).is_ok());
            let from_dn = headers
                .get("x-ssl-client-subject-dn")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|d| d == s);
            prop_assert!(from_ctx || from_dn, "subject {s:?} came from nowhere");
        }
    }
}
