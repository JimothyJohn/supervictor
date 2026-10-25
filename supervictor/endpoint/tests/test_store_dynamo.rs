//! `DeviceStore` contract against a REAL DynamoDB — DynamoDB Local, not a
//! mock. The SQLite suites (`test_store_sqlite`, `test_store_fleet`,
//! `test_store_concurrency`) pin the contract; this file replays it on the
//! production backend, plus what only DynamoDB can show: the TTL attribute
//! the table's `TimeToLiveSpecification` reaps, and the conditional-write
//! semantics under contention.
//!
//! Needs `DYNAMODB_LOCAL_ENDPOINT` (fails loudly without it — a suite that
//! silently passes when the database is absent proves nothing). Locally:
//!
//! ```text
//! docker run --rm -p 8000:8000 amazon/dynamodb-local:3.3.2
//! DYNAMODB_LOCAL_ENDPOINT=http://127.0.0.1:8000 \
//!     cargo test -p supervictor-endpoint --features dynamo
//! ```
//!
//! CI runs it in the `dynamo` job (service container, see ci.yml). Every
//! test creates its own uniquely named pair of tables, so tests run in
//! parallel without sharing rows.
//!
//! Fixture rows are dated 2099. DynamoDB Local really does run a TTL
//! reaper: rows whose `expires_at` is already in the past get deleted in
//! the background in batches of 20, so July-2026 rows with a 30-day TTL
//! vanished mid-test (2 of 8 runs lost exactly the 20 oldest rows, in
//! the container's file-backed `-sharedDb` mode). Not a store bug — the
//! real service would reap them too, just within 48h instead of seconds.
#![cfg(feature = "dynamo")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use aws_sdk_dynamodb::config::{Credentials, Region};
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
    ScalarAttributeType, TimeToLiveSpecification, TimeToLiveStatus,
};
use aws_sdk_dynamodb::Client;
use supervictor_endpoint::error::AppError;
use supervictor_endpoint::handlers::handle_uplink;
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::store::dynamo::DynamoDeviceStore;
use supervictor_endpoint::store::DeviceStore;
use supervictor_endpoint::time::parse_rfc3339_unix;

const ENDPOINT_VAR: &str = "DYNAMODB_LOCAL_ENDPOINT";
const TTL_DAYS: u32 = 30;
const THREADS: usize = 16;
const OPS_PER_THREAD: usize = 25;

fn endpoint() -> String {
    std::env::var(ENDPOINT_VAR).unwrap_or_else(|_| {
        panic!(
            "{ENDPOINT_VAR} is not set. This suite needs a running DynamoDB Local:\n\
             \x20 docker run --rm -p 8000:8000 amazon/dynamodb-local:3.3.2\n\
             \x20 {ENDPOINT_VAR}=http://127.0.0.1:8000 cargo test -p supervictor-endpoint --features dynamo\n\
             (CI provides it as a service container — .github/workflows/ci.yml, job `dynamo`)"
        )
    })
}

async fn sdk_config() -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .endpoint_url(endpoint())
        .region(Region::new("us-east-1"))
        // DynamoDB Local accepts any credentials; it keys its on-disk
        // database by access key, so keep the pair stable across tests.
        .credentials_provider(Credentials::new(
            "local",
            "local",
            None,
            None,
            "dynamodb-local",
        ))
        .load()
        .await
}

/// Two freshly created tables with the production key schema
/// (template.yaml) and a store on top of them.
struct Fixture {
    client: Client,
    devices: String,
    uplinks: String,
    store: Arc<dyn DeviceStore>,
}

impl Fixture {
    async fn new(tag: &str, ttl_days: u32) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let unique = format!(
            "{tag}-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let devices = format!("sv-test-devices-{unique}");
        let uplinks = format!("sv-test-uplinks-{unique}");

        let config = sdk_config().await;
        let client = Client::new(&config);

        client
            .create_table()
            .table_name(&devices)
            .billing_mode(BillingMode::PayPerRequest)
            .attribute_definitions(string_attr("device_id"))
            .key_schema(key("device_id", KeyType::Hash))
            .send()
            .await
            .expect("create devices table");
        client
            .create_table()
            .table_name(&uplinks)
            .billing_mode(BillingMode::PayPerRequest)
            .attribute_definitions(string_attr("device_id"))
            .attribute_definitions(string_attr("received_at"))
            .key_schema(key("device_id", KeyType::Hash))
            .key_schema(key("received_at", KeyType::Range))
            .send()
            .await
            .expect("create uplinks table");
        client
            .update_time_to_live()
            .table_name(&uplinks)
            .time_to_live_specification(
                TimeToLiveSpecification::builder()
                    .attribute_name("expires_at")
                    .enabled(true)
                    .build()
                    .unwrap(),
            )
            .send()
            .await
            .expect("enable TTL on uplinks table");

        let store = DynamoDeviceStore::new(&config, &devices, &uplinks, ttl_days);
        Self {
            client,
            devices,
            uplinks,
            store: Arc::new(store),
        }
    }

    /// Best-effort cleanup. In-memory DynamoDB Local forgets everything on
    /// exit anyway; this keeps a long-lived local instance from piling up
    /// tables. Skipped when a test panics — the failure is what matters.
    async fn teardown(self) {
        for table in [&self.devices, &self.uplinks] {
            let _ = self.client.delete_table().table_name(table).send().await;
        }
    }
}

fn string_attr(name: &str) -> AttributeDefinition {
    AttributeDefinition::builder()
        .attribute_name(name)
        .attribute_type(ScalarAttributeType::S)
        .build()
        .unwrap()
}

fn key(name: &str, key_type: KeyType) -> KeySchemaElement {
    KeySchemaElement::builder()
        .attribute_name(name)
        .key_type(key_type)
        .build()
        .unwrap()
}

fn device(id: &str, subject_dn: Option<&str>) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "owner-1".into(),
        subject_dn: subject_dn.map(str::to_string),
        status: "active".into(),
        created_at: "2099-07-01T00:00:00+00:00".into(),
        power_threshold_ma: None,
        style: Default::default(),
    }
}

fn uplink(id: &str, at: &str, current: i64) -> UplinkRecord {
    UplinkRecord {
        device_id: id.into(),
        received_at: at.into(),
        payload: serde_json::json!({ "current": current }),
    }
}

/// Run `f` on THREADS plain OS threads at once. The DynamoDB store bridges
/// to async with `Handle::current()`, so each thread enters the test
/// runtime's handle first — exactly what an axum worker thread has.
fn hammer<F>(store: &Arc<dyn DeviceStore>, f: F)
where
    F: Fn(&dyn DeviceStore, usize) + Send + Sync + 'static,
{
    let handle = tokio::runtime::Handle::current();
    let f = Arc::new(f);
    let barrier = Arc::new(Barrier::new(THREADS));
    let workers: Vec<_> = (0..THREADS)
        .map(|thread_id| {
            let store = Arc::clone(store);
            let barrier = Arc::clone(&barrier);
            let f = Arc::clone(&f);
            let handle = handle.clone();
            thread::spawn(move || {
                let _guard = handle.enter();
                barrier.wait();
                f(store.as_ref(), thread_id);
            })
        })
        .collect();
    tokio::task::block_in_place(|| {
        for worker in workers {
            worker.join().expect("worker thread panicked");
        }
    });
}

// ── Devices ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_round_trip_duplicate_and_missing() {
    let fx = Fixture::new("devices", TTL_DAYS).await;
    let store = fx.store.as_ref();

    let saved = store
        .put_device(device("dev-1", Some("CN=dev-1,OU=devices")))
        .unwrap();
    assert_eq!(saved.device_id, "dev-1");

    let fetched = store.get_device("dev-1").unwrap().unwrap();
    assert_eq!(fetched.owner_id, "owner-1");
    assert_eq!(fetched.status, "active");
    assert_eq!(fetched.subject_dn.as_deref(), Some("CN=dev-1,OU=devices"));
    assert_eq!(fetched.created_at, "2099-07-01T00:00:00+00:00");

    let err = store.put_device(device("dev-1", None)).unwrap_err();
    assert!(
        matches!(err, AppError::DeviceAlreadyExists { ref device_id } if device_id == "dev-1"),
        "conditional put must surface as DeviceAlreadyExists, got {err:?}"
    );
    // The losing write must not have clobbered the subject_dn.
    assert!(store
        .get_device("dev-1")
        .unwrap()
        .unwrap()
        .subject_dn
        .is_some());

    assert!(store.get_device("nonexistent").unwrap().is_none());

    let unbound = store.put_device(device("dev-2", None)).unwrap();
    assert!(unbound.subject_dn.is_none());
    assert!(store
        .get_device("dev-2")
        .unwrap()
        .unwrap()
        .subject_dn
        .is_none());
    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_devices_returns_every_row() {
    let fx = Fixture::new("list", TTL_DAYS).await;
    assert!(fx.store.list_devices().unwrap().is_empty());
    for i in 0..5 {
        fx.store
            .put_device(device(&format!("dev-{i}"), None))
            .unwrap();
    }
    let mut ids: Vec<String> = fx
        .store
        .list_devices()
        .unwrap()
        .into_iter()
        .map(|d| d.device_id)
        .collect();
    ids.sort();
    assert_eq!(ids, ["dev-0", "dev-1", "dev-2", "dev-3", "dev-4"]);
    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_device_status_updates_and_rejects_unknown() {
    let fx = Fixture::new("status", TTL_DAYS).await;
    fx.store
        .put_device(device("dev-1", Some("CN=dev-1")))
        .unwrap();

    let updated = fx.store.set_device_status("dev-1", "inactive").unwrap();
    assert_eq!(updated.status, "inactive");
    assert_eq!(
        updated.subject_dn.as_deref(),
        Some("CN=dev-1"),
        "ReturnValues ALL_NEW must carry the untouched fields"
    );
    assert_eq!(
        fx.store.get_device("dev-1").unwrap().unwrap().status,
        "inactive"
    );

    let err = fx.store.set_device_status("ghost", "inactive").unwrap_err();
    assert!(
        matches!(err, AppError::DeviceNotFound { .. }),
        "got {err:?}"
    );
    assert!(
        fx.store.get_device("ghost").unwrap().is_none(),
        "a failed conditional update must not upsert"
    );
    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_device_threshold_sets_clears_and_rejects_unknown() {
    let fx = Fixture::new("threshold", TTL_DAYS).await;
    fx.store
        .put_device(device("dev-1", Some("CN=dev-1")))
        .unwrap();
    assert_eq!(
        fx.store
            .get_device("dev-1")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        None,
        "no attribute = no override"
    );

    let updated = fx
        .store
        .set_device_threshold("dev-1", Some(12_000))
        .unwrap();
    assert_eq!(updated.power_threshold_ma, Some(12_000));
    assert_eq!(
        updated.subject_dn.as_deref(),
        Some("CN=dev-1"),
        "ReturnValues ALL_NEW must carry the untouched fields"
    );
    assert_eq!(
        fx.store
            .get_device("dev-1")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(12_000)
    );
    // Negative and huge values survive the N attribute round trip.
    for t in [-1, i64::MAX, i64::MIN] {
        let updated = fx.store.set_device_threshold("dev-1", Some(t)).unwrap();
        assert_eq!(updated.power_threshold_ma, Some(t), "{t}");
    }

    // Clearing REMOVEs the attribute rather than storing a null.
    let cleared = fx.store.set_device_threshold("dev-1", None).unwrap();
    assert_eq!(cleared.power_threshold_ma, None);
    let listed = fx.store.list_devices().unwrap();
    assert_eq!(listed[0].power_threshold_ma, None);

    // A threshold given at registration is stored with the item.
    let mut with = device("dev-2", None);
    with.power_threshold_ma = Some(5_000);
    fx.store.put_device(with).unwrap();
    assert_eq!(
        fx.store
            .get_device("dev-2")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(5_000)
    );

    let err = fx.store.set_device_threshold("ghost", Some(1)).unwrap_err();
    assert!(
        matches!(err, AppError::DeviceNotFound { .. }),
        "got {err:?}"
    );
    assert!(
        fx.store.get_device("ghost").unwrap().is_none(),
        "a failed conditional update must not upsert"
    );
    fx.teardown().await;
}

// ── Uplinks ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_device_style_sets_clears_and_rejects_unknown() {
    use supervictor_common::models::{DeviceStyle, LineStyle, Marker};
    let fx = Fixture::new("style", TTL_DAYS).await;
    let mut styled = device("dev-1", Some("CN=dev-1"));
    styled.style = DeviceStyle {
        color: Some("#ff8800".into()),
        line_style: Some(LineStyle::Dashed),
        marker: None,
    };
    fx.store.put_device(styled.clone()).unwrap();
    fx.store.put_device(device("dev-2", None)).unwrap();
    assert_eq!(
        fx.store.get_device("dev-1").unwrap().unwrap().style,
        styled.style,
        "style given at registration is stored with the item"
    );
    assert!(fx
        .store
        .get_device("dev-2")
        .unwrap()
        .unwrap()
        .style
        .is_default());

    // SET the present parts, REMOVE the absent ones, in one update.
    let updated = fx
        .store
        .set_device_style(
            "dev-1",
            DeviceStyle {
                color: None,
                line_style: Some(LineStyle::Dotted),
                marker: Some(Marker::Diamond),
            },
        )
        .unwrap();
    assert_eq!(updated.style.color, None, "REMOVEd");
    assert_eq!(updated.style.line_style, Some(LineStyle::Dotted));
    assert_eq!(updated.style.marker, Some(Marker::Diamond));
    assert_eq!(
        updated.subject_dn.as_deref(),
        Some("CN=dev-1"),
        "ALL_NEW carries the untouched fields"
    );
    let listed = fx.store.list_devices().unwrap();
    let dev1 = listed.iter().find(|d| d.device_id == "dev-1").unwrap();
    assert_eq!(dev1.style, updated.style);

    // All-absent is a pure REMOVE expression.
    let cleared = fx
        .store
        .set_device_style("dev-1", DeviceStyle::default())
        .unwrap();
    assert!(cleared.style.is_default());
    // All-present is a pure SET expression.
    let full = fx
        .store
        .set_device_style(
            "dev-1",
            DeviceStyle {
                color: Some("s7".into()),
                line_style: Some(LineStyle::DashDot),
                marker: Some(Marker::Triangle),
            },
        )
        .unwrap();
    assert_eq!(full.style.color.as_deref(), Some("s7"));
    assert_eq!(full.style.marker, Some(Marker::Triangle));

    let err = fx
        .store
        .set_device_style("ghost", DeviceStyle::default())
        .unwrap_err();
    assert!(
        matches!(err, AppError::DeviceNotFound { .. }),
        "got {err:?}"
    );
    assert!(
        fx.store.get_device("ghost").unwrap().is_none(),
        "a failed conditional update must not upsert"
    );
    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uplinks_are_newest_first_with_limit() {
    let fx = Fixture::new("uplinks", TTL_DAYS).await;
    assert!(fx.store.get_uplinks("dev-1", 10).unwrap().is_empty());
    for i in 0..5 {
        fx.store
            .put_uplink(uplink("dev-1", &format!("2099-07-01T00:00:0{i}+00:00"), i))
            .unwrap();
    }
    fx.store
        .put_uplink(uplink("dev-2", "2099-07-01T00:00:09+00:00", 99))
        .unwrap();

    let all = fx.store.get_uplinks("dev-1", 10).unwrap();
    assert_eq!(all.len(), 5, "other devices' rows must not leak in");
    assert_eq!(all[0].received_at, "2099-07-01T00:00:04+00:00");
    assert_eq!(all[0].payload["current"], 4);
    assert_eq!(all[4].received_at, "2099-07-01T00:00:00+00:00");

    let two = fx.store.get_uplinks("dev-1", 2).unwrap();
    assert_eq!(two.len(), 2);
    assert_eq!(two[1].payload["current"], 3);
    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn latest_uplinks_is_max_per_device_with_that_rows_payload() {
    let fx = Fixture::new("latest", TTL_DAYS).await;
    for id in ["dev-1", "dev-2", "dev-silent"] {
        fx.store.put_device(device(id, None)).unwrap();
    }
    for (at, fw) in [
        ("2099-07-01T10:00:00+00:00", "0.0.9"),
        ("2099-07-01T12:00:00+00:00", "0.1.0"),
        ("2099-07-01T11:00:00+00:00", "0.0.9"),
    ] {
        fx.store
            .put_uplink(UplinkRecord {
                device_id: "dev-1".into(),
                received_at: at.into(),
                payload: serde_json::json!({ "current": 1, "fw": fw }),
            })
            .unwrap();
    }
    fx.store
        .put_uplink(uplink("dev-2", "2099-06-30T00:00:00+00:00", 2))
        .unwrap();

    let mut latest: Vec<(String, String, String)> = fx
        .store
        .latest_uplinks()
        .unwrap()
        .into_iter()
        .map(|u| {
            let fw = u.payload["fw"].as_str().unwrap_or("-").to_string();
            (u.device_id, u.received_at, fw)
        })
        .collect();
    latest.sort();
    assert_eq!(
        latest,
        vec![
            (
                "dev-1".to_string(),
                "2099-07-01T12:00:00+00:00".to_string(),
                "0.1.0".to_string()
            ),
            (
                "dev-2".to_string(),
                "2099-06-30T00:00:00+00:00".to_string(),
                "-".to_string()
            ),
        ],
        "max per device, newest row's payload, silent devices absent"
    );
    fx.teardown().await;
}

/// What only the real backend can prove: the row carries the epoch the
/// table's TTL will reap it at, and the table actually has TTL enabled on
/// that attribute (the template.yaml contract).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uplink_rows_carry_the_ttl_expiry() {
    let fx = Fixture::new("ttl", TTL_DAYS).await;
    let received_at = "2099-07-16T00:00:00+00:00";
    fx.store
        .put_uplink(uplink("dev-1", received_at, 1))
        .unwrap();

    let item = fx
        .client
        .get_item()
        .table_name(&fx.uplinks)
        .key("device_id", AttributeValue::S("dev-1".into()))
        .key("received_at", AttributeValue::S(received_at.into()))
        .send()
        .await
        .unwrap()
        .item
        .expect("row present");
    let expires_at: u64 = item["expires_at"].as_n().unwrap().parse().unwrap();
    let expected = parse_rfc3339_unix(received_at).unwrap() + u64::from(TTL_DAYS) * 86_400;
    assert_eq!(
        expires_at, expected,
        "expires_at = received_at + {TTL_DAYS}d"
    );
    assert_eq!(item["payload"].as_s().unwrap(), r#"{"current":1}"#);

    let ttl = fx
        .client
        .describe_time_to_live()
        .table_name(&fx.uplinks)
        .send()
        .await
        .unwrap();
    let desc = ttl.time_to_live_description().expect("ttl description");
    assert_eq!(desc.attribute_name(), Some("expires_at"));
    assert_eq!(desc.time_to_live_status(), Some(&TimeToLiveStatus::Enabled));
    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ttl_disabled_stores_no_expiry() {
    let fx = Fixture::new("nottl", 0).await;
    let received_at = "2099-07-16T00:00:00+00:00";
    fx.store
        .put_uplink(uplink("dev-1", received_at, 1))
        .unwrap();
    let item = fx
        .client
        .get_item()
        .table_name(&fx.uplinks)
        .key("device_id", AttributeValue::S("dev-1".into()))
        .key("received_at", AttributeValue::S(received_at.into()))
        .send()
        .await
        .unwrap()
        .item
        .unwrap();
    assert!(
        !item.contains_key("expires_at"),
        "UPLINK_TTL_DAYS=0 must mean no expiry attribute, got {item:?}"
    );
    fx.teardown().await;
}

// ── Handler on the production backend ───────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identity_binding_holds_on_dynamo() {
    let fx = Fixture::new("identity", TTL_DAYS).await;
    fx.store
        .put_device(device("dev-1", Some("CN=dev-1,OU=devices")))
        .unwrap();
    let body = r#"{"id":"dev-1","current":7}"#;

    let err = handle_uplink(
        Some(body),
        Some("CN=dev-2,OU=devices".into()),
        Some(fx.store.as_ref()),
        false,
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::SubjectMismatch { .. }),
        "got {err:?}"
    );
    assert!(fx.store.get_uplinks("dev-1", 10).unwrap().is_empty());

    handle_uplink(
        Some(body),
        Some("OU=devices, CN=dev-1".into()),
        Some(fx.store.as_ref()),
        true,
    )
    .unwrap();
    assert_eq!(fx.store.get_uplinks("dev-1", 10).unwrap().len(), 1);
    fx.teardown().await;
}

// ── Concurrency ──────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_registration_of_same_id_has_exactly_one_winner() {
    let fx = Fixture::new("race-put", TTL_DAYS).await;
    let wins = Arc::new(AtomicUsize::new(0));
    let conflicts = Arc::new(AtomicUsize::new(0));
    let (w, c) = (Arc::clone(&wins), Arc::clone(&conflicts));
    hammer(&fx.store, move |store, thread_id| {
        match store.put_device(device("contested", Some(&format!("CN=t{thread_id}")))) {
            Ok(_) => w.fetch_add(1, Ordering::SeqCst),
            Err(AppError::DeviceAlreadyExists { .. }) => c.fetch_add(1, Ordering::SeqCst),
            Err(other) => panic!("unexpected error under contention: {other:?}"),
        };
    });
    assert_eq!(
        wins.load(Ordering::SeqCst),
        1,
        "exactly one conditional put wins"
    );
    assert_eq!(conflicts.load(Ordering::SeqCst), THREADS - 1);
    assert_eq!(fx.store.list_devices().unwrap().len(), 1);
    fx.teardown().await;
}

/// Uplink rows are keyed (device_id, received_at): distinct timestamps
/// must all land; nothing may be lost or double-counted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_uplinks_are_all_persisted() {
    let fx = Fixture::new("race-uplink", TTL_DAYS).await;
    fx.store.put_device(device("sensor", None)).unwrap();
    hammer(&fx.store, |store, thread_id| {
        for i in 0..OPS_PER_THREAD {
            store
                .put_uplink(UplinkRecord {
                    device_id: "sensor".into(),
                    received_at: format!("2099-07-11T{thread_id:02}:{i:02}:00+00:00"),
                    payload: serde_json::json!({ "current": thread_id * 1000 + i }),
                })
                .expect("uplink put must not fail under contention");
        }
    });
    // On failure, say which rows vanished and how many the table really
    // holds (paginated raw query) — a store truncation and a backend
    // deletion look identical from get_uplinks alone.
    let raw_total = fx
        .client
        .query()
        .table_name(&fx.uplinks)
        .key_condition_expression("device_id = :did")
        .expression_attribute_values(":did", AttributeValue::S("sensor".into()))
        .into_paginator()
        .items()
        .send()
        .collect::<Result<Vec<_>, _>>()
        .await
        .unwrap()
        .len();
    let rows = fx
        .store
        .get_uplinks("sensor", THREADS * OPS_PER_THREAD + 1)
        .unwrap();
    let mut missing: Vec<(usize, usize)> = Vec::new();
    for t in 0..THREADS {
        for i in 0..OPS_PER_THREAD {
            let at = format!("2099-07-11T{t:02}:{i:02}:00+00:00");
            if !rows.iter().any(|r| r.received_at == at) {
                missing.push((t, i));
            }
        }
    }
    assert_eq!(
        rows.len(),
        THREADS * OPS_PER_THREAD,
        "get_uplinks returned {} rows; table holds {raw_total}; missing (thread, op): {missing:?}",
        rows.len(),
    );
    let mut currents: Vec<i64> = rows
        .iter()
        .map(|u| u.payload["current"].as_i64().unwrap())
        .collect();
    currents.sort_unstable();
    currents.dedup();
    assert_eq!(
        currents.len(),
        THREADS * OPS_PER_THREAD,
        "torn write detected"
    );
    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_status_flips_end_in_a_valid_state() {
    let fx = Fixture::new("race-status", TTL_DAYS).await;
    fx.store.put_device(device("toggled", None)).unwrap();
    hammer(&fx.store, |store, thread_id| {
        let status = if thread_id % 2 == 0 {
            "active"
        } else {
            "inactive"
        };
        for _ in 0..OPS_PER_THREAD / 5 {
            let updated = store
                .set_device_status("toggled", status)
                .expect("status update must not fail under contention");
            assert!(updated.status == "active" || updated.status == "inactive");
            assert_eq!(updated.device_id, "toggled");
            assert_eq!(updated.owner_id, "owner-1", "update must not drop fields");
        }
    });
    let final_status = fx.store.get_device("toggled").unwrap().unwrap().status;
    assert!(final_status == "active" || final_status == "inactive");
    assert_eq!(fx.store.list_devices().unwrap().len(), 1);
    fx.teardown().await;
}

// ── Window queries for the history views (sort-key range on the real table) ──

async fn seed_window(fx: &Fixture) {
    fx.store.put_device(device("dev-1", None)).unwrap();
    fx.store.put_device(device("dev-2", None)).unwrap();
    for (id, at) in [
        ("dev-1", "2099-07-01T09:59:59+00:00"),
        ("dev-1", "2099-07-01T10:00:00+00:00"),
        ("dev-2", "2099-07-01T10:00:00.250+00:00"),
        ("dev-1", "2099-07-01T10:05:00+00:00"),
        ("dev-2", "2099-07-01T10:10:00+00:00"),
        ("dev-1", "2099-07-01T10:15:00+00:00"),
    ] {
        fx.store.put_uplink(uplink(id, at, 1)).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uplinks_since_matches_the_sqlite_contract() {
    let fx = Fixture::new("since", TTL_DAYS).await;
    seed_window(&fx).await;

    let rows = fx
        .store
        .uplinks_since("2099-07-01T10:00:00+00:00", 100)
        .unwrap();
    let got: Vec<(String, String)> = rows
        .into_iter()
        .map(|u| (u.received_at, u.device_id))
        .collect();
    assert_eq!(
        got,
        vec![
            ("2099-07-01T10:00:00+00:00".to_string(), "dev-1".to_string()),
            ("2099-07-01T10:00:00.250+00:00".into(), "dev-2".into()),
            ("2099-07-01T10:05:00+00:00".into(), "dev-1".into()),
            ("2099-07-01T10:10:00+00:00".into(), "dev-2".into()),
            ("2099-07-01T10:15:00+00:00".into(), "dev-1".into()),
        ]
    );

    let capped = fx
        .store
        .uplinks_since("2099-07-01T10:00:00+00:00", 2)
        .unwrap();
    let got: Vec<String> = capped.into_iter().map(|u| u.received_at).collect();
    assert_eq!(
        got,
        vec![
            "2099-07-01T10:00:00+00:00".to_string(),
            "2099-07-01T10:00:00.250+00:00".to_string(),
        ],
        "cap keeps the oldest rows across devices"
    );
    assert!(fx
        .store
        .uplinks_since("2099-07-01T10:00:00+00:00", 0)
        .unwrap()
        .is_empty());
    assert!(fx
        .store
        .uplinks_since("2100-01-01T00:00:00+00:00", 100)
        .unwrap()
        .is_empty());

    fx.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_uplinks_since_is_a_sort_key_range() {
    let fx = Fixture::new("dsince", TTL_DAYS).await;
    seed_window(&fx).await;

    let rows = fx
        .store
        .device_uplinks_since("dev-2", "2099-07-01T10:00:00+00:00", 100)
        .unwrap();
    let got: Vec<String> = rows.iter().map(|u| u.received_at.clone()).collect();
    assert_eq!(
        got,
        vec![
            "2099-07-01T10:00:00.250+00:00".to_string(),
            "2099-07-01T10:10:00+00:00".to_string(),
        ]
    );
    let capped = fx
        .store
        .device_uplinks_since("dev-1", "2099-07-01T00:00:00+00:00", 1)
        .unwrap();
    assert_eq!(capped.len(), 1);
    assert_eq!(capped[0].received_at, "2099-07-01T09:59:59+00:00");
    assert!(fx
        .store
        .device_uplinks_since("ghost", "2099-07-01T00:00:00+00:00", 100)
        .unwrap()
        .is_empty());
    // A hostile `since` is a key value, not an expression.
    for hostile in ["' OR 1=1 --", "\u{0}", "a AND received_at >= :x"] {
        let _ = fx
            .store
            .device_uplinks_since("dev-1", hostile, 100)
            .unwrap();
    }

    fx.teardown().await;
}
