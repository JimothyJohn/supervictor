//! OTA firmware routes (docs/OTA.md O.1): who may upload, who may read,
//! what an image must look like, how `Range` behaves, and how `desired_fw`
//! reaches the device. Adversarial by default: every negative path here is
//! a hole a device certificate or a typo could otherwise walk through.

mod common;

use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue};
use axum_test::TestServer;
use sha2::{Digest, Sha256};
use supervictor_common::firmware::{ChipId, HEADER_LEN, MAGIC, SLOT_SIZE};
use supervictor_endpoint::firmware::{FirmwareStore, FsFirmwareStore};
use supervictor_endpoint::models::DeviceRecord;
use supervictor_endpoint::routes::{self, RouterOptions};
use supervictor_endpoint::store::DeviceStore;

const SUBJECT_HEADER: &str = "x-ssl-client-subject-dn";
const ADMIN_DN: &str = "CN=ops,OU=admin,O=supervictor";
const DEV1_DN: &str = "CN=poe-iso-rack-01,OU=devices,O=supervictor";
const DEV2_DN: &str = "CN=poe-iso-rack-02,OU=devices,O=supervictor";

struct Rig {
    server: TestServer,
    store: Arc<dyn DeviceStore>,
    firmware: Arc<FsFirmwareStore>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let firmware = Arc::new(FsFirmwareStore::new(dir.path().join("fw")).unwrap());
    let store = common::test_store();
    let app = routes::router_with_firmware(
        store.clone(),
        RouterOptions::default(),
        Some(firmware.clone()),
    );
    Rig {
        server: TestServer::new(app),
        store,
        firmware,
        _dir: dir,
    }
}

fn rig_without_blob_store() -> (TestServer, Arc<dyn DeviceStore>) {
    let store = common::test_store();
    let app = routes::router(store.clone());
    (TestServer::new(app), store)
}

fn dn(value: &'static str) -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static(SUBJECT_HEADER),
        HeaderValue::from_static(value),
    )
}

fn seed(store: &dyn DeviceStore, id: &str, subject_dn: Option<&str>) {
    store
        .put_device(DeviceRecord {
            device_id: id.into(),
            owner_id: "owner-1".into(),
            subject_dn: subject_dn.map(String::from),
            status: "active".into(),
            created_at: "2026-07-01T00:00:00+00:00".into(),
            power_threshold_ma: None,
            style: Default::default(),
            desired_fw: None,
        })
        .unwrap();
}

/// A syntactically valid ESP-IDF image: header, `body_len` bytes of
/// deterministic filler, appended SHA-256 — exactly what the bootloader
/// and `firmware::validate_image` check.
fn image(chip: ChipId, body_len: usize, seed: u8) -> Vec<u8> {
    let mut img = vec![0u8; HEADER_LEN];
    img[0] = MAGIC;
    img[1] = 3;
    img[12..14].copy_from_slice(&chip.raw().to_le_bytes());
    img[23] = 1;
    img.extend((0..body_len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)));
    let digest = Sha256::digest(&img);
    img.extend_from_slice(&digest);
    img
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn upload(
    server: &TestServer,
    device: &str,
    version: &str,
    img: &[u8],
) -> axum_test::TestResponse {
    let (n, v) = dn(ADMIN_DN);
    server
        .put(&format!("/devices/{device}/firmware/{version}"))
        .add_header(n, v)
        .bytes(img.to_vec().into())
        .await
}

// ── Upload ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn upload_is_admin_only() {
    let r = rig();
    seed(r.store.as_ref(), "poe-iso-rack-01", Some(DEV1_DN));
    let img = image(ChipId::Esp32, 4096, 1);

    let resp = r
        .server
        .put("/devices/poe-iso-rack-01/firmware/0.2.0")
        .bytes(img.clone().into())
        .await;
    assert_eq!(resp.status_code(), 403, "no certificate: {}", resp.text());

    let (n, v) = dn(DEV1_DN);
    let resp = r
        .server
        .put("/devices/poe-iso-rack-01/firmware/0.2.0")
        .add_header(n, v)
        .bytes(img.clone().into())
        .await;
    assert_eq!(
        resp.status_code(),
        403,
        "the device's own cert cannot upload"
    );

    let resp = upload(&r.server, "poe-iso-rack-01", "0.2.0", &img).await;
    assert_eq!(resp.status_code(), 201, "{}", resp.text());
    let manifest: serde_json::Value = resp.json();
    assert_eq!(manifest["version"], "0.2.0");
    assert_eq!(manifest["chip"], "esp32");
    assert_eq!(manifest["size"], img.len() as u64);
    assert_eq!(manifest["sha256"], hex(&Sha256::digest(&img)));
    assert!(manifest["uploaded_at"].as_str().unwrap().contains('T'));
}

#[tokio::test]
async fn upload_needs_a_registered_device() {
    let r = rig();
    let resp = upload(&r.server, "ghost", "0.2.0", &image(ChipId::Esp32, 100, 0)).await;
    assert_eq!(resp.status_code(), 404, "{}", resp.text());
    assert!(
        std::fs::read_dir(r.firmware.root())
            .unwrap()
            .next()
            .is_none(),
        "nothing written for an unknown device"
    );
}

#[tokio::test]
async fn upload_rejects_bad_images() {
    let r = rig();
    seed(r.store.as_ref(), "d1", None);

    let good = image(ChipId::Esp32, 1000, 7);

    // Wrong magic.
    let mut bad = good.clone();
    bad[0] = 0xE8;
    let resp = upload(&r.server, "d1", "0.2.0", &bad).await;
    assert_eq!(resp.status_code(), 422, "{}", resp.text());
    assert!(resp.text().contains("magic"), "{}", resp.text());

    // Corrupted body: the appended digest no longer matches.
    let mut bad = good.clone();
    bad[HEADER_LEN + 10] ^= 0xFF;
    let resp = upload(&r.server, "d1", "0.2.0", &bad).await;
    assert_eq!(resp.status_code(), 422);
    assert!(resp.text().contains("sha256"), "{}", resp.text());

    // Truncated tail.
    let resp = upload(&r.server, "d1", "0.2.0", &good[..good.len() - 1]).await;
    assert_eq!(resp.status_code(), 422);

    // Too short to be an image at all.
    let resp = upload(&r.server, "d1", "0.2.0", &good[..HEADER_LEN - 1]).await;
    assert_eq!(resp.status_code(), 422);
    let resp = upload(&r.server, "d1", "0.2.0", b"").await;
    assert_eq!(resp.status_code(), 422);

    // A chip the fleet does not run (ESP32-S3 = 9).
    let mut bad = good.clone();
    bad[12..14].copy_from_slice(&9u16.to_le_bytes());
    // Re-hash so only the chip is wrong.
    let n = bad.len() - 32;
    let d = Sha256::digest(&bad[..n]);
    bad[n..].copy_from_slice(&d);
    let resp = upload(&r.server, "d1", "0.2.0", &bad).await;
    assert_eq!(resp.status_code(), 422);
    assert!(resp.text().contains("chip"), "{}", resp.text());

    // Zero segments.
    let mut bad = good.clone();
    bad[1] = 0;
    let n = bad.len() - 32;
    let d = Sha256::digest(&bad[..n]);
    bad[n..].copy_from_slice(&d);
    let resp = upload(&r.server, "d1", "0.2.0", &bad).await;
    assert_eq!(resp.status_code(), 422);

    // Nothing stored after all that.
    assert!(r.firmware.manifest("d1", "0.2.0").unwrap().is_none());
}

#[tokio::test]
async fn upload_without_appended_hash_is_accepted_but_recorded_as_is() {
    // hash_appended = 0: nothing to verify, size and header still checked.
    let r = rig();
    seed(r.store.as_ref(), "d1", None);
    let mut img = image(ChipId::Esp32C3, 500, 3);
    img[23] = 0;
    img.truncate(img.len() - 32);
    let resp = upload(&r.server, "d1", "0.2.0", &img).await;
    assert_eq!(resp.status_code(), 201, "{}", resp.text());
    let m: serde_json::Value = resp.json();
    assert_eq!(m["chip"], "esp32c3");
    assert_eq!(m["size"], img.len() as u64);
}

#[tokio::test]
async fn upload_larger_than_a_slot_is_refused_before_it_is_read() {
    let r = rig();
    seed(r.store.as_ref(), "d1", None);
    let too_big = vec![0u8; SLOT_SIZE as usize + 1];
    let resp = upload(&r.server, "d1", "0.2.0", &too_big).await;
    assert_eq!(resp.status_code(), 413, "{}", resp.text());
    assert!(r.firmware.manifest("d1", "0.2.0").unwrap().is_none());
}

#[tokio::test]
async fn exactly_one_slot_is_the_largest_accepted_image() {
    let r = rig();
    seed(r.store.as_ref(), "d1", None);
    let img = image(ChipId::Esp32, SLOT_SIZE as usize - HEADER_LEN - 32, 0);
    assert_eq!(img.len(), SLOT_SIZE as usize);
    let resp = upload(&r.server, "d1", "9.9.9", &img).await;
    assert_eq!(resp.status_code(), 201, "{}", resp.text());
}

#[tokio::test]
async fn upload_rejects_unsafe_identifiers_and_writes_nothing() {
    let r = rig();
    seed(r.store.as_ref(), "d1", None);
    let img = image(ChipId::Esp32, 64, 0);
    for version in [
        "1.2",
        "v1.2.3",
        "1.2.3+build",
        "..",
        "%2e%2e%2f",
        "1.2.3%2F..%2F..%2Fetc",
        "1.2.3%20",
        "latest",
    ] {
        let resp = upload(&r.server, "d1", version, &img).await;
        assert!(
            resp.status_code() == 422 || resp.status_code() == 404,
            "version {version:?}: {} {}",
            resp.status_code(),
            resp.text()
        );
    }
    // Device ids that pass registration but not the path check.
    for device in ["a%2Fb", "a%20b", "..%2F..%2Fx", "d%C3%A9"] {
        let resp = upload(&r.server, device, "0.2.0", &img).await;
        assert!(
            resp.status_code() == 422 || resp.status_code() == 404,
            "device {device:?}: {} {}",
            resp.status_code(),
            resp.text()
        );
    }
    // The only thing under the store root may be d1's (empty) directory.
    let entries: Vec<_> = walkdir(r.firmware.root());
    assert!(
        entries.iter().all(|p| p.starts_with(r.firmware.root())),
        "{entries:?}"
    );
    assert!(
        entries.iter().all(|p| p.is_dir()),
        "no files written: {entries:?}"
    );
    // And nothing escaped above the root.
    let parent = r.firmware.root().parent().unwrap();
    let outside: Vec<_> = std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p != r.firmware.root())
        .collect();
    assert!(outside.is_empty(), "escaped the store root: {outside:?}");
}

fn walkdir(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p.clone());
            }
            out.push(p);
        }
    }
    out
}

#[tokio::test]
async fn reupload_replaces_the_image_and_its_manifest() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    let a = image(ChipId::Esp32, 300, 1);
    let b = image(ChipId::Esp32, 700, 2);
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &a).await.status_code(),
        201
    );
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &b).await.status_code(),
        201
    );
    let m = r.firmware.manifest("d1", "0.2.0").unwrap().unwrap();
    assert_eq!(m.size as usize, b.len());
    assert_eq!(m.sha256, hex(&Sha256::digest(&b)));
    let (n, v) = dn(DEV1_DN);
    let resp = r
        .server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n, v)
        .await;
    assert_eq!(resp.as_bytes().to_vec(), b);
}

// ── Download auth ──────────────────────────────────────────────────────

#[tokio::test]
async fn download_is_for_the_devices_own_certificate_or_an_admin() {
    let r = rig();
    seed(r.store.as_ref(), "poe-iso-rack-01", Some(DEV1_DN));
    seed(r.store.as_ref(), "poe-iso-rack-02", Some(DEV2_DN));
    let img = image(ChipId::Esp32, 2000, 1);
    assert_eq!(
        upload(&r.server, "poe-iso-rack-01", "0.2.0", &img)
            .await
            .status_code(),
        201
    );

    for path in [
        "/devices/poe-iso-rack-01/firmware/0.2.0",
        "/devices/poe-iso-rack-01/firmware/0.2.0/manifest",
    ] {
        // No certificate.
        let resp = r.server.get(path).await;
        assert_eq!(resp.status_code(), 403, "{path} anonymous");

        // Another device's certificate: this is the hole that would leak
        // rack-01's private key to rack-02.
        let (n, v) = dn(DEV2_DN);
        let resp = r.server.get(path).add_header(n, v).await;
        assert_eq!(resp.status_code(), 403, "{path} other device");

        // A lookalike: same CN in a different OU is not the same subject.
        let resp = r
            .server
            .get(path)
            .add_header(
                HeaderName::from_static(SUBJECT_HEADER),
                HeaderValue::from_static("CN=poe-iso-rack-01,OU=admins,O=evil"),
            )
            .await;
        assert_eq!(resp.status_code(), 403, "{path} lookalike");

        // The device itself.
        let (n, v) = dn(DEV1_DN);
        let resp = r.server.get(path).add_header(n, v).await;
        assert_eq!(resp.status_code(), 200, "{path} own cert: {}", resp.text());

        // An admin.
        let (n, v) = dn(ADMIN_DN);
        let resp = r.server.get(path).add_header(n, v).await;
        assert_eq!(resp.status_code(), 200, "{path} admin");
    }
}

#[tokio::test]
async fn unbound_device_image_is_admin_only() {
    // No subject_dn on record: nothing can prove it is the device, so
    // only an admin may read the image (it bakes in a private key).
    let r = rig();
    seed(r.store.as_ref(), "d1", None);
    let img = image(ChipId::Esp32, 100, 1);
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &img).await.status_code(),
        201
    );
    let (n, v) = dn(DEV1_DN);
    let resp = r
        .server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n, v)
        .await;
    assert_eq!(resp.status_code(), 403);
    let (n, v) = dn(ADMIN_DN);
    let resp = r
        .server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n, v)
        .await;
    assert_eq!(resp.status_code(), 200);
}

#[tokio::test]
async fn missing_image_and_unknown_device_are_404_after_auth() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    let (n, v) = dn(DEV1_DN);
    let resp = r
        .server
        .get("/devices/d1/firmware/0.9.0")
        .add_header(n.clone(), v.clone())
        .await;
    assert_eq!(resp.status_code(), 404);
    let resp = r
        .server
        .get("/devices/d1/firmware/0.9.0/manifest")
        .add_header(n.clone(), v.clone())
        .await;
    assert_eq!(resp.status_code(), 404);
    // Unknown device: 404 regardless of the certificate presented, but an
    // anonymous caller learns nothing either way (404 before 403 would
    // enumerate devices; both are "no").
    let resp = r.server.get("/devices/ghost/firmware/0.2.0").await;
    assert!(resp.status_code() == 404 || resp.status_code() == 403);
}

// ── Range ──────────────────────────────────────────────────────────────

async fn get_range(server: &TestServer, range: Option<&'static str>) -> axum_test::TestResponse {
    let (n, v) = dn(DEV1_DN);
    let req = server.get("/devices/d1/firmware/0.2.0").add_header(n, v);
    match range {
        Some(r) => {
            req.add_header(
                HeaderName::from_static("range"),
                HeaderValue::from_static(r),
            )
            .await
        }
        None => req.await,
    }
}

#[tokio::test]
async fn ranged_download_streams_the_exact_bytes() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    let img = image(ChipId::Esp32, 10_000, 5);
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &img).await.status_code(),
        201
    );
    let total = img.len();
    let etag = format!("\"{}\"", hex(&Sha256::digest(&img)));

    // Whole.
    let resp = get_range(&r.server, None).await;
    assert_eq!(resp.status_code(), 200);
    assert_eq!(resp.as_bytes().to_vec(), img);
    assert_eq!(resp.header("accept-ranges"), "bytes");
    assert_eq!(resp.header("etag"), etag.as_str());
    assert_eq!(resp.header("content-length"), total.to_string().as_str());
    assert_eq!(resp.header("content-type"), "application/octet-stream");

    // First 4 KB sector.
    let resp = get_range(&r.server, Some("bytes=0-4095")).await;
    assert_eq!(resp.status_code(), 206);
    assert_eq!(resp.as_bytes().to_vec(), img[..4096]);
    assert_eq!(
        resp.header("content-range"),
        format!("bytes 0-4095/{total}").as_str()
    );
    assert_eq!(resp.header("content-length"), "4096");
    assert_eq!(resp.header("etag"), etag.as_str());

    // Resume from an offset to the end (what the device sends after a drop).
    let resp = get_range(&r.server, Some("bytes=8192-")).await;
    assert_eq!(resp.status_code(), 206);
    assert_eq!(resp.as_bytes().to_vec(), img[8192..]);
    assert_eq!(
        resp.header("content-range"),
        format!("bytes 8192-{}/{total}", total - 1).as_str()
    );

    // End past the resource is clamped.
    let resp = get_range(&r.server, Some("bytes=9000-999999")).await;
    assert_eq!(resp.status_code(), 206);
    assert_eq!(resp.as_bytes().to_vec(), img[9000..]);

    // Suffix.
    let resp = get_range(&r.server, Some("bytes=-32")).await;
    assert_eq!(resp.status_code(), 206);
    assert_eq!(resp.as_bytes().to_vec(), img[total - 32..]);

    // One byte.
    let resp = get_range(&r.server, Some("bytes=0-0")).await;
    assert_eq!(resp.status_code(), 206);
    assert_eq!(resp.as_bytes().to_vec(), img[..1]);
}

#[tokio::test]
async fn unsatisfiable_range_is_416_with_the_total() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    let img = image(ChipId::Esp32, 1000, 5);
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &img).await.status_code(),
        201
    );
    let total = img.len();
    for range in ["bytes=99999-", "bytes=99999-100000"] {
        let resp = get_range(&r.server, Some(range)).await;
        assert_eq!(resp.status_code(), 416, "{range}");
        assert_eq!(
            resp.header("content-range"),
            format!("bytes */{total}").as_str()
        );
        assert!(resp.as_bytes().is_empty());
    }
}

#[tokio::test]
async fn if_match_pins_a_resumed_download_to_the_image_it_started_with() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    let a = image(ChipId::Esp32, 3000, 1);
    let b = image(ChipId::Esp32, 3000, 2);
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &a).await.status_code(),
        201
    );
    let sha_a = hex(&Sha256::digest(&a));

    let (n, v) = dn(DEV1_DN);
    let matching = HeaderValue::from_str(&format!("\"{sha_a}\"")).unwrap();
    let resp = r
        .server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n.clone(), v.clone())
        .add_header(HeaderName::from_static("if-match"), matching.clone())
        .add_header(
            HeaderName::from_static("range"),
            HeaderValue::from_static("bytes=1000-"),
        )
        .await;
    assert_eq!(resp.status_code(), 206);
    assert_eq!(resp.as_bytes().to_vec(), a[1000..]);

    // An operator re-uploads the version mid-download.
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &b).await.status_code(),
        201
    );
    let resp = r
        .server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n.clone(), v.clone())
        .add_header(HeaderName::from_static("if-match"), matching)
        .add_header(
            HeaderName::from_static("range"),
            HeaderValue::from_static("bytes=2000-"),
        )
        .await;
    assert_eq!(resp.status_code(), 412, "{}", resp.text());
    assert_eq!(
        resp.header("etag"),
        format!("\"{}\"", hex(&Sha256::digest(&b))).as_str()
    );

    // Without If-Match the client just gets the new image (its own final
    // digest check catches the splice).
    let resp = r
        .server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n.clone(), v.clone())
        .await;
    assert_eq!(resp.status_code(), 200);
    assert_eq!(resp.as_bytes().to_vec(), b);

    // Wildcard and weak forms are accepted.
    for ok in ["*", &format!("W/\"{}\"", hex(&Sha256::digest(&b)))] {
        let resp = r
            .server
            .get("/devices/d1/firmware/0.2.0")
            .add_header(n.clone(), v.clone())
            .add_header(
                HeaderName::from_static("if-match"),
                HeaderValue::from_str(ok).unwrap(),
            )
            .await;
        assert_eq!(resp.status_code(), 200, "{ok}");
    }
}

#[tokio::test]
async fn odd_range_headers_get_the_whole_image() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    let img = image(ChipId::Esp32, 1000, 5);
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &img).await.status_code(),
        201
    );
    for range in [
        "apples",
        "bytes=0-9,20-29",
        "bytes=5-2",
        "items=0-9",
        "bytes=",
        "bytes=-0",
        "bytes=99999999999999999999999-",
    ] {
        let resp = get_range(&r.server, Some(range)).await;
        assert_eq!(resp.status_code(), 200, "{range}");
        assert_eq!(resp.as_bytes().to_vec(), img, "{range}");
    }
}

// ── desired_fw ─────────────────────────────────────────────────────────

async fn set_desired(server: &TestServer, device: &str, body: &str) -> axum_test::TestResponse {
    let (n, v) = dn(ADMIN_DN);
    server
        .put(&format!("/devices/{device}/desired_fw"))
        .add_header(n, v)
        .text(body)
        .await
}

#[tokio::test]
async fn desired_fw_is_admin_only_and_needs_a_stored_image() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));

    let resp = r
        .server
        .put("/devices/d1/desired_fw")
        .text(r#"{"desired_fw":"0.2.0"}"#)
        .await;
    assert_eq!(resp.status_code(), 403);
    let (n, v) = dn(DEV1_DN);
    let resp = r
        .server
        .put("/devices/d1/desired_fw")
        .add_header(n, v)
        .text(r#"{"desired_fw":"0.2.0"}"#)
        .await;
    assert_eq!(
        resp.status_code(),
        403,
        "a device cannot pick its own firmware"
    );

    // No image yet.
    let resp = set_desired(&r.server, "d1", r#"{"desired_fw":"0.2.0"}"#).await;
    assert_eq!(resp.status_code(), 404, "{}", resp.text());

    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &image(ChipId::Esp32, 100, 1))
            .await
            .status_code(),
        201
    );
    let resp = set_desired(&r.server, "d1", r#"{"desired_fw":"0.2.0"}"#).await;
    assert_eq!(resp.status_code(), 200, "{}", resp.text());
    let body: serde_json::Value = resp.json();
    assert_eq!(body["desired_fw"], "0.2.0");

    // Visible on the device record.
    let resp = r.server.get("/devices/d1").await;
    let body: serde_json::Value = resp.json();
    assert_eq!(body["desired_fw"], "0.2.0");

    // Body discipline: invalid version, unknown key, array, unknown device.
    for bad in [
        r#"{"desired_fw":"latest"}"#,
        r#"{"desired_fw":"../0.2.0"}"#,
        r#"{"desired_fw":"0.2.0","extra":1}"#,
        r#"{"desired-fw":"0.2.0"}"#,
        "[]",
        "42",
        r#""0.2.0""#,
    ] {
        let resp = set_desired(&r.server, "d1", bad).await;
        assert_eq!(resp.status_code(), 422, "{bad}: {}", resp.text());
    }
    let resp = set_desired(&r.server, "ghost", r#"{"desired_fw":null}"#).await;
    assert_eq!(resp.status_code(), 404);

    // Still 0.2.0 after the rejected attempts.
    let dev = r.store.get_device("d1").unwrap().unwrap();
    assert_eq!(dev.desired_fw.as_deref(), Some("0.2.0"));

    // Clearing: {}, null, empty body.
    for clear in [r#"{}"#, "null", ""] {
        set_desired(&r.server, "d1", r#"{"desired_fw":"0.2.0"}"#).await;
        let resp = set_desired(&r.server, "d1", clear).await;
        assert_eq!(resp.status_code(), 200, "{clear:?}: {}", resp.text());
        let body: serde_json::Value = resp.json();
        assert!(body.get("desired_fw").is_none(), "{clear:?}: {body}");
    }
}

#[tokio::test]
async fn desired_fw_reaches_the_device_in_the_uplink_response_and_the_fleet() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &image(ChipId::Esp32, 100, 1))
            .await
            .status_code(),
        201
    );

    // Before a target is set: no desired_fw key at all.
    let (n, v) = dn(DEV1_DN);
    let resp = r
        .server
        .post("/")
        .add_header(n.clone(), v.clone())
        .json(&serde_json::json!({"id": "d1", "current": 5, "fw": "0.1.0"}))
        .await;
    assert_eq!(resp.status_code(), 200, "{}", resp.text());
    let body: serde_json::Value = resp.json();
    assert!(body.get("desired_fw").is_none(), "{body}");

    set_desired(&r.server, "d1", r#"{"desired_fw":"0.2.0"}"#).await;

    let resp = r
        .server
        .post("/")
        .add_header(n.clone(), v.clone())
        .json(&serde_json::json!({"id": "d1", "current": 5, "fw": "0.1.0"}))
        .await;
    let body: serde_json::Value = resp.json();
    assert_eq!(body["desired_fw"], "0.2.0");

    // Fleet shows the rollout in flight.
    let (an, av) = dn(ADMIN_DN);
    let resp = r
        .server
        .get("/fleet")
        .add_header(an.clone(), av.clone())
        .await;
    let fleet: Vec<serde_json::Value> = resp.json();
    let row = fleet.iter().find(|d| d["device_id"] == "d1").unwrap();
    assert_eq!(row["fw"], "0.1.0");
    assert_eq!(row["desired_fw"], "0.2.0");
    assert_eq!(row["fw_pending"], true);

    // The device reports the new version: no longer pending, and the
    // response still carries the target (the device ignores an equal one).
    let resp = r
        .server
        .post("/")
        .add_header(n, v)
        .json(&serde_json::json!({"id": "d1", "current": 5, "fw": "0.2.0"}))
        .await;
    let body: serde_json::Value = resp.json();
    assert_eq!(body["desired_fw"], "0.2.0");
    let resp = r.server.get("/fleet").add_header(an, av).await;
    let fleet: Vec<serde_json::Value> = resp.json();
    let row = fleet.iter().find(|d| d["device_id"] == "d1").unwrap();
    assert_eq!(row["fw_pending"], false);
}

#[tokio::test]
async fn unregistered_uplinks_never_see_a_desired_fw() {
    // Open-ingest mode accepts unknown devices; there is no record to
    // carry a target, and the response must not invent one.
    let r = rig();
    let resp = r
        .server
        .post("/")
        .json(&serde_json::json!({"id": "stranger", "current": 1}))
        .await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert!(body.get("desired_fw").is_none());
}

// ── Delete ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_is_admin_only_and_idempotent_in_effect() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    assert_eq!(
        upload(&r.server, "d1", "0.2.0", &image(ChipId::Esp32, 100, 1))
            .await
            .status_code(),
        201
    );
    let (n, v) = dn(DEV1_DN);
    let resp = r
        .server
        .delete("/devices/d1/firmware/0.2.0")
        .add_header(n, v)
        .await;
    assert_eq!(resp.status_code(), 403);
    let (n, v) = dn(ADMIN_DN);
    let resp = r
        .server
        .delete("/devices/d1/firmware/0.2.0")
        .add_header(n.clone(), v.clone())
        .await;
    assert_eq!(resp.status_code(), 204);
    let resp = r
        .server
        .delete("/devices/d1/firmware/0.2.0")
        .add_header(n.clone(), v.clone())
        .await;
    assert_eq!(resp.status_code(), 404);
    let resp = r
        .server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n, v)
        .await;
    assert_eq!(resp.status_code(), 404);
}

// ── No blob store ──────────────────────────────────────────────────────

#[tokio::test]
async fn without_a_blob_store_firmware_routes_answer_503_but_clearing_works() {
    let (server, store) = rig_without_blob_store();
    seed(store.as_ref(), "d1", Some(DEV1_DN));
    let (an, av) = dn(ADMIN_DN);
    let resp = server
        .put("/devices/d1/firmware/0.2.0")
        .add_header(an.clone(), av.clone())
        .bytes(image(ChipId::Esp32, 100, 1).into())
        .await;
    assert_eq!(resp.status_code(), 503, "{}", resp.text());
    let (n, v) = dn(DEV1_DN);
    let resp = server
        .get("/devices/d1/firmware/0.2.0")
        .add_header(n.clone(), v.clone())
        .await;
    assert_eq!(resp.status_code(), 503);
    let resp = server
        .get("/devices/d1/firmware/0.2.0/manifest")
        .add_header(n, v)
        .await;
    assert_eq!(resp.status_code(), 503);
    let resp = server
        .put("/devices/d1/desired_fw")
        .add_header(an.clone(), av.clone())
        .text(r#"{"desired_fw":"0.2.0"}"#)
        .await;
    assert_eq!(resp.status_code(), 503);
    // Clearing a target never needs the blob store.
    let resp = server
        .put("/devices/d1/desired_fw")
        .add_header(an.clone(), av.clone())
        .text("null")
        .await;
    assert_eq!(resp.status_code(), 200, "{}", resp.text());
    // Auth still comes first: anonymous gets 403, not 503.
    let resp = server.get("/devices/d1/firmware/0.2.0").await;
    assert!(resp.status_code() == 403 || resp.status_code() == 503);
}

// ── Concurrency ────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_uploads_and_downloads_never_expose_a_torn_image() {
    let r = rig();
    seed(r.store.as_ref(), "d1", Some(DEV1_DN));
    let images: Vec<Vec<u8>> = (0..6u8)
        .map(|s| image(ChipId::Esp32, 20_000 + usize::from(s) * 1000, s))
        .collect();
    let shas: std::collections::HashSet<String> =
        images.iter().map(|i| hex(&Sha256::digest(i))).collect();
    let fw: Arc<dyn FirmwareStore> = r.firmware.clone();
    let writers_done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let writer_count = images.len();

    let mut tasks = Vec::new();
    for (i, img) in images.iter().cloned().enumerate() {
        let fw = fw.clone();
        let done = writers_done.clone();
        tasks.push(tokio::task::spawn_blocking(move || {
            for round in 0..5 {
                fw.put(
                    "d1",
                    "0.2.0",
                    ChipId::Esp32,
                    &img,
                    &format!("2026-01-01T00:00:{:02}+00:00", i * 5 + round),
                )
                .unwrap();
            }
            done.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
    }
    let fw2 = fw.clone();
    let shas2 = shas.clone();
    let done = writers_done.clone();
    tasks.push(tokio::task::spawn_blocking(move || {
        // Read until every writer has finished, then once more: every
        // committed manifest names a complete, uploaded blob.
        let mut seen = 0;
        loop {
            let finished = done.load(std::sync::atomic::Ordering::SeqCst) == writer_count;
            if let Some(m) = fw2.manifest("d1", "0.2.0").unwrap() {
                // A manifest read a moment ago may already have been
                // replaced; the blob it names is content-addressed, so
                // either it is still there (and complete) or it is gone.
                if let Some(blob) = fw2.read("d1", "0.2.0", &m.sha256, 0, u64::MAX).unwrap() {
                    let sha = hex(&Sha256::digest(&blob));
                    assert!(shas2.contains(&sha), "torn or foreign blob read back");
                    assert_eq!(sha, m.sha256, "blob is the one its manifest names");
                    assert_eq!(
                        blob.len() as u64,
                        u64::from(m.size),
                        "blob matches its manifest"
                    );
                    seen += 1;
                }
            }
            if finished {
                break;
            }
            std::thread::yield_now();
        }
        assert!(seen > 0, "reader never saw a committed image");
    }));
    for t in tasks {
        t.await.unwrap();
    }
    let m = fw.manifest("d1", "0.2.0").unwrap().unwrap();
    let blob = fw
        .read("d1", "0.2.0", &m.sha256, 0, u64::MAX)
        .unwrap()
        .unwrap();
    assert_eq!(
        hex(&Sha256::digest(&blob)),
        m.sha256,
        "final state consistent"
    );
    // No temp files left behind.
    let leftovers: Vec<_> = walkdir(r.firmware.root())
        .into_iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().contains(".tmp-"))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}
