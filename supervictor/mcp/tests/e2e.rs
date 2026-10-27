//! End to end, nothing mocked: the real endpoint with in-binary mTLS and
//! certificates minted by the repo's `gen_certs.sh`, the real
//! `supervictor-mcp` binary as a child process, JSON-RPC over its stdio.
//!
//! Pins: the MCP lifecycle, every tool's shape against live data, the
//! identity rules (a device certificate is not an admin; a foreign CA never
//! handshakes), argument validation, and that stdout carries nothing but
//! JSON lines.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::{json, Value};
use supervictor_endpoint::models::{DeviceRecord, DeviceStyle, UplinkRecord};
use supervictor_endpoint::routes::router;
use supervictor_endpoint::store::sqlite::SqliteDeviceStore;
use supervictor_endpoint::store::DeviceStore;
use supervictor_endpoint::time::now_rfc3339;
use supervictor_endpoint::tls::{server_config, TlsListener, TlsPaths, TlsPeer};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

// ── PKI fixture (as the endpoint's own TLS test mints it) ────────────

struct Pki {
    dir: PathBuf,
}

impl Pki {
    fn mint(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sv-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pki = Self { dir };
        pki.gen(&["ca"]);
        pki.gen(&["device", "dev-1"]);
        pki.gen(&["admin", "ops"]);
        pki.gen(&["server", "local", "localhost"]);
        pki
    }

    fn gen(&self, args: &[&str]) {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../endpoint/scripts/gen_certs.sh");
        let out = Command::new("bash")
            .arg(&script)
            .args(args)
            .env("CERTS_DIR", &self.dir)
            .current_dir(&self.dir)
            .output()
            .expect("spawn bash");
        assert!(
            out.status.success(),
            "gen_certs.sh {args:?} failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn ca(&self) -> PathBuf {
        self.dir.join("ca/ca.pem")
    }
    fn device(&self, name: &str) -> (PathBuf, PathBuf) {
        let d = self.dir.join("devices").join(name);
        (d.join("client.pem"), d.join("client.key"))
    }
    fn admin(&self, name: &str) -> (PathBuf, PathBuf) {
        let d = self.dir.join("admins").join(name);
        (d.join("admin.pem"), d.join("admin.key"))
    }
    fn server_paths(&self) -> TlsPaths {
        let d = self.dir.join("servers/local");
        TlsPaths {
            cert: d.join("server.pem"),
            key: d.join("server.key"),
            client_ca: self.ca(),
        }
    }
}

async fn start_server(pki: &Pki, store: Arc<dyn DeviceStore>) -> std::net::SocketAddr {
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listener = TlsListener::from_parts(tcp, server_config(&pki.server_paths()).unwrap());
    let addr = listener.socket_addr();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(store).into_make_service_with_connect_info::<TlsPeer>(),
        )
        .await
        .unwrap();
    });
    addr
}

/// Two registered devices, one of them with a fresh uplink, plus one whose
/// id needs percent-encoding on the wire.
fn seed(store: &dyn DeviceStore) {
    for (id, owner) in [("dev-1", "alice"), ("dev-2", "bob"), ("odd id,1", "carol")] {
        store
            .put_device(DeviceRecord {
                device_id: id.into(),
                owner_id: owner.into(),
                subject_dn: Some(format!("CN={id},O=Supervictor,OU=devices")),
                status: "active".into(),
                created_at: now_rfc3339(),
                power_threshold_ma: None,
                min_on_s: None,
                style: DeviceStyle::default(),
                desired_fw: None,
            })
            .unwrap();
    }
    store
        .put_uplink(UplinkRecord {
            device_id: "dev-1".into(),
            received_at: now_rfc3339(),
            payload: json!({"id": "dev-1", "current": 1500, "seq": 3, "fw": "0.1.0", "uptime_s": 42}),
        })
        .unwrap();
}

/// A port nothing listens on: bound and released by this process.
fn closed_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

// ── MCP child process driver ─────────────────────────────────────────

struct Mcp {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    next_id: u64,
    /// Every raw stdout line, to assert the transport rule at the end.
    raw: Vec<String>,
}

impl Mcp {
    fn spawn(url: &str, ca: &Path, cert: &Path, key: &Path) -> Self {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_supervictor-mcp"))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("SUPERVICTOR_URL", url)
            .env("SUPERVICTOR_CA", ca)
            .env("SUPERVICTOR_ADMIN_CERT", cert)
            .env("SUPERVICTOR_ADMIN_KEY", key)
            .env("SUPERVICTOR_TIMEOUT_SECS", "5")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn supervictor-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            raw: Vec::new(),
        }
    }

    async fn send_raw(&mut self, line: &str) {
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.write_all(b"\n").await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn recv(&mut self) -> Value {
        let line =
            tokio::time::timeout(std::time::Duration::from_secs(20), self.stdout.next_line())
                .await
                .expect("mcp reply within 20s")
                .unwrap()
                .expect("mcp closed stdout");
        self.raw.push(line.clone());
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("non-JSON on stdout ({e}): {line}"))
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send_raw(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
        )
        .await;
        let reply = self.recv().await;
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    async fn initialize(&mut self) -> Value {
        let r = self
            .request(
                "initialize",
                json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "0"}}),
            )
            .await;
        self.send_raw(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await;
        r
    }

    /// `tools/call` → the result object (`content`, `isError`, …).
    async fn call(&mut self, name: &str, args: Value) -> Value {
        let r = self
            .request("tools/call", json!({"name": name, "arguments": args}))
            .await;
        assert!(r.get("error").is_none(), "protocol error: {r}");
        r["result"].clone()
    }

    async fn finish(mut self) {
        drop(self.stdin);
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), self.child.wait())
            .await
            .expect("mcp exits after stdin closes")
            .unwrap();
        assert!(status.success(), "mcp exit status {status}");
        for line in &self.raw {
            assert!(
                serde_json::from_str::<Value>(line).is_ok() && !line.contains('\n'),
                "stdout must carry only JSON lines: {line}"
            );
        }
    }
}

fn text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().unwrap_or("")
}

// ── tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn admin_identity_sees_everything() {
    let pki = Pki::mint("admin");
    let store: Arc<dyn DeviceStore> = Arc::new(SqliteDeviceStore::new(":memory:").unwrap());
    seed(store.as_ref());
    let addr = start_server(&pki, store).await;
    let url = format!("https://localhost:{}", addr.port());
    let (cert, key) = pki.admin("ops");
    let mut mcp = Mcp::spawn(&url, &pki.ca(), &cert, &key);

    // Lifecycle.
    let init = mcp.initialize().await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "{init}"
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "supervictor");
    let ping = mcp.request("ping", json!({})).await;
    assert_eq!(ping["result"], json!({}));
    let list = mcp.request("tools/list", json!({})).await;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "supervictor_backend_status",
            "supervictor_fleet_summary",
            "supervictor_fleet_status",
            "supervictor_device_status",
            "supervictor_fleet_history",
        ]
    );

    // Backend status: reachable, healthy, admin accepted, fleet headline.
    let r = mcp.call("supervictor_backend_status", json!({})).await;
    assert_eq!(r["isError"], false, "{r}");
    let s = &r["structuredContent"];
    assert_eq!(s["reachable"], true, "{s}");
    assert_eq!(s["healthy"], true, "{s}");
    assert_eq!(s["admin"]["authorized"], true, "{s}");
    assert_eq!(s["fleet"]["total"], 3, "{s}");
    assert_eq!(s["fleet"]["fresh"], 1, "{s}");
    assert!(
        s["tls_version"].as_str().unwrap().starts_with("TLSv1_"),
        "{s}"
    );
    assert!(s["latency_ms"].is_u64(), "{s}");
    assert!(
        text(&r).contains("healthy") && text(&r).contains("admin certificate accepted"),
        "{r}"
    );

    // Summary.
    let r = mcp.call("supervictor_fleet_summary", json!({})).await;
    assert_eq!(r["structuredContent"]["summary"]["active"], 3, "{r}");
    assert_eq!(r["structuredContent"]["summary"]["on"], 1, "{r}");
    assert!(text(&r).contains("3 devices"), "{r}");

    // Per-device status: worst first (never-heard before fresh).
    let r = mcp.call("supervictor_fleet_status", json!({})).await;
    let devices = r["structuredContent"]["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 3, "{r}");
    assert_eq!(devices[0]["staleness"], "unknown", "{r}");
    assert_eq!(devices[2]["device_id"], "dev-1", "{r}");
    assert_eq!(devices[2]["staleness"], "fresh");
    assert_eq!(devices[2]["power"], "on");
    assert_eq!(devices[2]["current"], 1500);
    assert_eq!(r["structuredContent"]["by_staleness"]["unknown"], 2, "{r}");
    assert!(text(&r).contains("1500 mA"), "{r}");

    // Filters compose.
    let r = mcp
        .call(
            "supervictor_fleet_status",
            json!({"staleness": "fresh", "owner_id": "alice"}),
        )
        .await;
    assert_eq!(r["structuredContent"]["matched"], 1, "{r}");
    assert_eq!(r["structuredContent"]["total"], 3, "{r}");
    let r = mcp
        .call(
            "supervictor_fleet_status",
            json!({"id_prefix": "dev-", "power": "unknown"}),
        )
        .await;
    assert_eq!(r["structuredContent"]["matched"], 1, "{r}");
    assert_eq!(
        r["structuredContent"]["devices"][0]["device_id"], "dev-2",
        "{r}"
    );
    let r = mcp
        .call("supervictor_fleet_status", json!({"status": "bogus"}))
        .await;
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("active, inactive"), "{r}");

    // One device, with its record, health row and uplinks.
    let r = mcp
        .call("supervictor_device_status", json!({"device_id": "dev-1"}))
        .await;
    assert_eq!(r["isError"], false, "{r}");
    let s = &r["structuredContent"];
    assert_eq!(s["device"]["owner_id"], "alice", "{s}");
    assert_eq!(s["health"]["staleness"], "fresh", "{s}");
    assert_eq!(s["recent_uplinks"][0]["payload"]["seq"], 3, "{s}");
    assert!(text(&r).contains("seq 3"), "{r}");
    let r = mcp
        .call(
            "supervictor_device_status",
            json!({"device_id": "dev-2", "uplinks": false}),
        )
        .await;
    assert_eq!(r["structuredContent"]["recent_uplinks"], Value::Null, "{r}");
    assert_eq!(
        r["structuredContent"]["health"]["staleness"], "unknown",
        "{r}"
    );
    // An id with a space and a comma survives the route.
    let r = mcp
        .call(
            "supervictor_device_status",
            json!({"device_id": "odd id,1"}),
        )
        .await;
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["device"]["owner_id"], "carol", "{r}");
    // Unknown ids are a clear error, and traversal-shaped ids are just unknown.
    for id in ["ghost", "../fleet", "dev-1/uplinks"] {
        let r = mcp
            .call("supervictor_device_status", json!({"device_id": id}))
            .await;
        assert_eq!(r["isError"], true, "{id}: {r}");
        assert!(text(&r).contains("not registered"), "{id}: {r}");
    }
    let r = mcp.call("supervictor_device_status", json!({})).await;
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("device_id"), "{r}");

    // History.
    let r = mcp
        .call("supervictor_fleet_history", json!({"window": "1h"}))
        .await;
    assert_eq!(r["isError"], false, "{r}");
    let h = &r["structuredContent"]["history"];
    assert_eq!(h["window"], "1h", "{h}");
    assert_eq!(h["devices"][0]["device_id"], "dev-1", "{h}");
    assert!(h["devices"][0]["points"].is_array(), "{h}");
    let r = mcp
        .call(
            "supervictor_fleet_history",
            json!({"device_id": "dev-1", "summary_only": true}),
        )
        .await;
    assert!(
        r["structuredContent"]["history"]["devices"][0]
            .get("points")
            .is_none(),
        "{r}"
    );
    let r = mcp
        .call("supervictor_fleet_history", json!({"device_id": "dev-2"}))
        .await;
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("no telemetry"), "{r}");
    let r = mcp
        .call("supervictor_fleet_history", json!({"window": "2h"}))
        .await;
    assert_eq!(r["isError"], true, "{r}");

    // Endpoint override: a port nothing listens on is reported, not thrown.
    let dead = format!("https://localhost:{}", closed_port());
    let r = mcp
        .call("supervictor_backend_status", json!({"endpoint": dead}))
        .await;
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["reachable"], false, "{r}");
    assert!(
        text(&r).contains("UNREACHABLE") && text(&r).contains("listening"),
        "{r}"
    );
    let r = mcp
        .call("supervictor_fleet_summary", json!({"endpoint": dead}))
        .await;
    assert_eq!(r["isError"], true, "{r}");
    let r = mcp
        .call(
            "supervictor_fleet_summary",
            json!({"endpoint": "http://localhost:1"}),
        )
        .await;
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("https"), "{r}");

    // Protocol-level errors.
    let r = mcp
        .request("tools/call", json!({"name": "supervictor_nope"}))
        .await;
    assert_eq!(r["error"]["code"], -32602, "{r}");
    let r = mcp.request("resources/list", json!({})).await;
    assert_eq!(r["error"]["code"], -32601, "{r}");
    mcp.send_raw("{this is not json").await;
    let r = mcp.recv().await;
    assert_eq!(r["error"]["code"], -32700, "{r}");
    assert_eq!(r["id"], Value::Null, "{r}");

    mcp.finish().await;
}

#[tokio::test]
async fn device_identity_is_not_an_admin() {
    let pki = Pki::mint("device");
    let store: Arc<dyn DeviceStore> = Arc::new(SqliteDeviceStore::new(":memory:").unwrap());
    seed(store.as_ref());
    let addr = start_server(&pki, store).await;
    let url = format!("https://localhost:{}", addr.port());
    let (cert, key) = pki.device("dev-1");
    let mut mcp = Mcp::spawn(&url, &pki.ca(), &cert, &key);
    mcp.initialize().await;

    let r = mcp.call("supervictor_backend_status", json!({})).await;
    let s = &r["structuredContent"];
    assert_eq!(s["reachable"], true, "{s}");
    assert_eq!(s["healthy"], true, "{s}");
    assert_eq!(s["admin"]["authorized"], false, "{s}");
    assert_eq!(s["admin"]["http_status"], 403, "{s}");
    assert!(s.get("fleet").is_none(), "{s}");
    assert!(text(&r).contains("NOT accepted"), "{r}");

    for tool in [
        "supervictor_fleet_summary",
        "supervictor_fleet_status",
        "supervictor_fleet_history",
    ] {
        let r = mcp.call(tool, json!({})).await;
        assert_eq!(r["isError"], true, "{tool}: {r}");
        assert!(text(&r).contains("OU=admin"), "{tool}: {r}");
    }
    // /devices/{id} is a device route, but the health row needs /fleet.
    let r = mcp
        .call("supervictor_device_status", json!({"device_id": "dev-1"}))
        .await;
    assert_eq!(r["isError"], true, "{r}");
    mcp.finish().await;
}

#[tokio::test]
async fn foreign_ca_and_missing_identity_are_explained() {
    let pki = Pki::mint("foreign");
    let other = Pki::mint("foreign-other");
    let store: Arc<dyn DeviceStore> = Arc::new(SqliteDeviceStore::new(":memory:").unwrap());
    let addr = start_server(&pki, store).await;
    let url = format!("https://localhost:{}", addr.port());

    // Trusting the wrong CA: the server certificate does not chain.
    let (cert, key) = other.admin("ops");
    let mut mcp = Mcp::spawn(&url, &other.ca(), &cert, &key);
    mcp.initialize().await;
    let r = mcp.call("supervictor_backend_status", json!({})).await;
    assert_eq!(r["structuredContent"]["reachable"], false, "{r}");
    assert!(text(&r).contains("SUPERVICTOR_CA"), "{r}");
    mcp.finish().await;

    // Right CA, admin issued by the other CA: handshake or 403, either way
    // the message points at the certificate.
    let mut mcp = Mcp::spawn(&url, &pki.ca(), &cert, &key);
    mcp.initialize().await;
    let r = mcp.call("supervictor_fleet_summary", json!({})).await;
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("certificate"), "{r}");
    mcp.finish().await;

    // Certificate files that do not exist: the server still initializes
    // (so the model can read the problem) and every call names the path.
    let missing = pki.dir.join("missing.pem");
    let mut mcp = Mcp::spawn(&url, &pki.ca(), &missing, &missing);
    let init = mcp.initialize().await;
    assert!(init.get("result").is_some(), "{init}");
    let r = mcp.call("supervictor_fleet_summary", json!({})).await;
    assert_eq!(r["isError"], true, "{r}");
    assert!(
        text(&r).contains("missing.pem") && text(&r).contains("SUPERVICTOR_ADMIN_CERT"),
        "{r}"
    );
    let r = mcp.call("supervictor_backend_status", json!({})).await;
    assert_eq!(r["structuredContent"]["reachable"], false, "{r}");
    mcp.finish().await;
}
