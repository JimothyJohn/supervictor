//! Concurrency stress: N real threads hammering the store, asserting no
//! torn writes, lost updates, or duplicate rows. Runs against real SQLite
//! (both in-memory and file-backed — the file path is what production
//! containers use).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use supervictor_endpoint::error::AppError;
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::store::sqlite::SqliteDeviceStore;
use supervictor_endpoint::store::DeviceStore;

const THREADS: usize = 16;
const OPS_PER_THREAD: usize = 25;

fn device(id: &str) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "owner-1".into(),
        subject_dn: None,
        status: "active".into(),
        created_at: "2026-07-11T00:00:00+00:00".into(),
        power_threshold_ma: None,
        style: Default::default(),
        desired_fw: None,
    }
}

/// Run `f` on THREADS threads simultaneously (barrier-released) and wait.
fn hammer<F>(store: &Arc<dyn DeviceStore>, f: F)
where
    F: Fn(&dyn DeviceStore, usize) + Send + Sync + 'static,
{
    let f = Arc::new(f);
    let barrier = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|thread_id| {
            let store = Arc::clone(store);
            let barrier = Arc::clone(&barrier);
            let f = Arc::clone(&f);
            thread::spawn(move || {
                barrier.wait(); // maximize interleaving
                f(store.as_ref(), thread_id);
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("worker thread panicked");
    }
}

#[test]
fn concurrent_registration_of_same_id_has_exactly_one_winner() {
    let store = common::test_store();
    let wins = Arc::new(AtomicUsize::new(0));
    let conflicts = Arc::new(AtomicUsize::new(0));

    let (wins_ref, conflicts_ref) = (Arc::clone(&wins), Arc::clone(&conflicts));
    hammer(&store, move |store, _| {
        match store.put_device(device("contested")) {
            Ok(_) => wins_ref.fetch_add(1, Ordering::SeqCst),
            Err(AppError::DeviceAlreadyExists { .. }) => {
                conflicts_ref.fetch_add(1, Ordering::SeqCst)
            }
            Err(other) => panic!("unexpected error under contention: {other:?}"),
        };
    });

    assert_eq!(
        wins.load(Ordering::SeqCst),
        1,
        "exactly one insert must win"
    );
    assert_eq!(conflicts.load(Ordering::SeqCst), THREADS - 1);
    assert_eq!(store.list_devices().unwrap().len(), 1, "no duplicate rows");
}

#[test]
fn concurrent_registration_of_distinct_ids_loses_nothing() {
    let store = common::test_store();
    hammer(&store, |store, thread_id| {
        for i in 0..OPS_PER_THREAD {
            store
                .put_device(device(&format!("dev-{thread_id}-{i}")))
                .expect("distinct ids must all insert");
        }
    });
    assert_eq!(
        store.list_devices().unwrap().len(),
        THREADS * OPS_PER_THREAD,
        "every registration must be durable"
    );
}

#[test]
fn concurrent_uplinks_are_all_persisted() {
    let store = common::test_store();
    store.put_device(device("sensor")).unwrap();

    hammer(&store, |store, thread_id| {
        for i in 0..OPS_PER_THREAD {
            store
                .put_uplink(UplinkRecord {
                    device_id: "sensor".into(),
                    received_at: format!("2026-07-11T{:02}:{:02}:00+00:00", thread_id, i),
                    payload: serde_json::json!({ "current": thread_id * 1000 + i }),
                })
                .expect("uplink insert must not fail under contention");
        }
    });

    let uplinks = store
        .get_uplinks("sensor", THREADS * OPS_PER_THREAD + 1)
        .unwrap();
    assert_eq!(
        uplinks.len(),
        THREADS * OPS_PER_THREAD,
        "no uplink may be lost or double-counted"
    );

    // Every payload distinct — a torn/duplicated write would collide.
    let mut currents: Vec<i64> = uplinks
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
}

#[test]
fn concurrent_status_flips_end_in_a_valid_state() {
    let store = common::test_store();
    store.put_device(device("toggled")).unwrap();

    hammer(&store, |store, thread_id| {
        let status = if thread_id % 2 == 0 {
            "active"
        } else {
            "inactive"
        };
        for _ in 0..OPS_PER_THREAD {
            let updated = store
                .set_device_status("toggled", status)
                .expect("status update must not fail under contention");
            // The returned record must be internally consistent — never a
            // half-applied row.
            assert!(
                updated.status == "active" || updated.status == "inactive",
                "torn status read: {}",
                updated.status
            );
            assert_eq!(updated.device_id, "toggled");
        }
    });

    let final_status = store.get_device("toggled").unwrap().unwrap().status;
    assert!(
        final_status == "active" || final_status == "inactive",
        "final status must be one of the written values: {final_status}"
    );
    assert_eq!(store.list_devices().unwrap().len(), 1);
}

/// Same contested-insert race against a FILE-backed database — the
/// production configuration (SQLITE_DB_PATH), where SQLite's own locking
/// is in play, not just our Mutex.
#[test]
fn file_backed_store_survives_the_same_races() {
    let path = std::env::temp_dir().join(format!("sv-stress-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store: Arc<dyn DeviceStore> =
        Arc::new(SqliteDeviceStore::new(path.to_str().unwrap()).unwrap());

    let wins = Arc::new(AtomicUsize::new(0));
    let wins_ref = Arc::clone(&wins);
    hammer(&store, move |store, thread_id| {
        if store.put_device(device("contested")).is_ok() {
            wins_ref.fetch_add(1, Ordering::SeqCst);
        }
        for i in 0..OPS_PER_THREAD {
            store
                .put_uplink(UplinkRecord {
                    device_id: "contested".into(),
                    received_at: format!("2026-07-11T{:02}:{:02}:30+00:00", thread_id, i),
                    payload: serde_json::json!({ "current": thread_id * 1000 + i }),
                })
                .expect("file-backed uplink insert failed");
        }
    });

    assert_eq!(wins.load(Ordering::SeqCst), 1);
    assert_eq!(
        store
            .get_uplinks("contested", THREADS * OPS_PER_THREAD + 1)
            .unwrap()
            .len(),
        THREADS * OPS_PER_THREAD
    );

    std::fs::remove_file(&path).ok();
}
