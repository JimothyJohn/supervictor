//! The tools: each one HTTPS GET against the admin API, then shaping.
//!
//! Every tool is read-only and idempotent. Every tool takes an optional
//! `endpoint` (an `https://` URL) so one MCP server can be asked about a
//! local Quickstart endpoint, a staging box and production in the same
//! conversation; the admin identity is the same for all of them, so they
//! must share a fleet CA (the `./Quickstart certs` layout).
//!
//! Network failures surface as tool errors with the fix spelled out, except
//! in `supervictor_backend_status`, where "unreachable, because …" is the
//! answer rather than a failure.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use supervictor_common::routes;

use crate::client::{excerpt, Client, Fetched};
use crate::config::{Config, Target};
use crate::protocol::{ToolError, ToolFuture, ToolOutput, Tools};

/// `?window=` presets the endpoint accepts (`endpoint::history::Window`).
pub const WINDOWS: &[&str] = &["1m", "5m", "15m", "1h", "6h", "24h", "7d"];
/// Lifecycle statuses.
pub const STATUSES: &[&str] = &["active", "inactive"];
/// Staleness classes, in alert order (worst first).
pub const STALENESS: &[&str] = &["dark", "stale", "unknown", "fresh"];
/// Power states.
pub const POWER: &[&str] = &["on", "off", "unknown"];
/// The firmware's identifier contract (`HString<64>`, validated on ingest).
pub const MAX_DEVICE_ID_BYTES: usize = 64;

/// The fleet tool set.
pub struct FleetTools {
    config: Config,
    /// Built once; a bad certificate path becomes a per-call error so the
    /// server still initializes and the model can read what to fix.
    client: Result<Client, String>,
}

impl FleetTools {
    /// Load the admin identity named in `config`.
    pub fn new(config: Config) -> Self {
        let client = Client::new(&config);
        Self { config, client }
    }

    fn client(&self) -> Result<&Client, ToolError> {
        self.client.as_ref().map_err(|e| ToolError(e.clone()))
    }

    /// The `endpoint` argument, else the configured default.
    fn target(&self, args: &Map<String, Value>) -> Result<Target, ToolError> {
        match args.get("endpoint") {
            None | Some(Value::Null) => Ok(self.config.target.clone()),
            Some(Value::String(url)) => Target::parse(url).map_err(ToolError),
            Some(_) => Err("`endpoint` must be a string URL".into()),
        }
    }

    /// GET + status check + JSON decode, with the endpoint named in errors.
    async fn get_json(&self, target: &Target, route: &str) -> Result<(Fetched, Value), ToolError> {
        let fetched = self.client()?.get(target, route).await?;
        let status = fetched.response.status;
        if status == 403 {
            return Err(format!(
                "{} refused the admin identity for {route} (HTTP 403: {}); the certificate must \
                 carry OU=admin and be registered in the endpoint's client CA — issue one with \
                 ./Quickstart certs admin <name> and point SUPERVICTOR_ADMIN_CERT/KEY at it",
                target.display(),
                excerpt(&fetched.response.body)
            )
            .into());
        }
        if status == 404 {
            return Err(format!(
                "{}{route}: HTTP 404 — not found ({})",
                target.display(),
                excerpt(&fetched.response.body)
            )
            .into());
        }
        if !(200..300).contains(&status) {
            return Err(format!(
                "{}{route}: HTTP {status}: {}",
                target.display(),
                excerpt(&fetched.response.body)
            )
            .into());
        }
        let value = fetched.json()?;
        Ok((fetched, value))
    }

    async fn backend_status(&self, args: Map<String, Value>) -> Result<ToolOutput, ToolError> {
        let target = self.target(&args)?;
        let endpoint = target.display();
        let client = match self.client() {
            Ok(c) => c,
            Err(ToolError(e)) => {
                return Ok(status_output(
                    &endpoint,
                    json!({
                        "endpoint": endpoint,
                        "reachable": false,
                        "error": e,
                    }),
                ));
            }
        };
        let health = match client.get(&target, routes::HEALTH).await {
            Ok(f) => f,
            Err(e) => {
                return Ok(status_output(
                    &endpoint,
                    json!({
                        "endpoint": endpoint,
                        "reachable": false,
                        "error": e,
                    }),
                ));
            }
        };
        let healthy = health.response.status == 200
            && health
                .json()
                .ok()
                .and_then(|v| v.get("status").and_then(Value::as_str).map(|s| s == "ok"))
                .unwrap_or(false);
        // The admin probe doubles as the fleet headline.
        let (admin, summary) = match client.get(&target, routes::FLEET_SUMMARY).await {
            Ok(f) if f.response.status == 200 => (
                json!({ "authorized": true, "http_status": 200 }),
                f.json().ok(),
            ),
            Ok(f) => (
                json!({
                    "authorized": false,
                    "http_status": f.response.status,
                    "detail": excerpt(&f.response.body),
                }),
                None,
            ),
            Err(e) => (json!({ "authorized": false, "detail": e }), None),
        };
        let mut status = json!({
            "endpoint": endpoint,
            "reachable": true,
            "healthy": healthy,
            "health": {
                "http_status": health.response.status,
                "body": health.json().unwrap_or_else(|_| Value::String(excerpt(&health.response.body))),
            },
            "tls_version": health.tls_version,
            "latency_ms": health.latency_ms,
            "server": health.response.header("server"),
            "admin": admin,
        });
        if let Some(summary) = summary {
            status["fleet"] = summary;
        }
        Ok(status_output(&endpoint, status))
    }

    async fn fleet_summary(&self, args: Map<String, Value>) -> Result<ToolOutput, ToolError> {
        let target = self.target(&args)?;
        let (_, summary) = self.get_json(&target, routes::FLEET_SUMMARY).await?;
        let text = format!("{}\n{}", target.display(), summary_line(&summary));
        Ok(ToolOutput {
            structured: json!({ "endpoint": target.display(), "summary": summary }),
            text: Some(text),
        })
    }

    async fn fleet_status(&self, args: Map<String, Value>) -> Result<ToolOutput, ToolError> {
        let target = self.target(&args)?;
        let status = optional_enum(&args, "status", STATUSES)?;
        let staleness = optional_enum(&args, "staleness", STALENESS)?;
        let power = optional_enum(&args, "power", POWER)?;
        let owner = optional_string(&args, "owner_id")?;
        let prefix = optional_string(&args, "id_prefix")?;

        let (_, fleet) = self.get_json(&target, routes::FLEET).await?;
        let all = fleet.as_array().ok_or_else(|| {
            ToolError(format!("{}/fleet did not return a list", target.display()))
        })?;
        let mut devices: Vec<&Value> = all
            .iter()
            .filter(|d| status.is_none_or(|s| field(d, "status") == s))
            .filter(|d| staleness.is_none_or(|s| field(d, "staleness") == s))
            .filter(|d| power.is_none_or(|p| field(d, "power") == p))
            .filter(|d| owner.as_deref().is_none_or(|o| field(d, "owner_id") == o))
            .filter(|d| {
                prefix
                    .as_deref()
                    .is_none_or(|p| field(d, "device_id").starts_with(p))
            })
            .collect();
        // Worst first, then by id: a model asking "anything wrong?" reads
        // the top of the list.
        devices.sort_by_key(|d| (severity(d), field(d, "device_id").to_string()));

        let mut by_staleness: BTreeMap<&str, usize> = BTreeMap::new();
        let mut by_power: BTreeMap<&str, usize> = BTreeMap::new();
        for d in &devices {
            *by_staleness.entry(field(d, "staleness")).or_default() += 1;
            *by_power.entry(field(d, "power")).or_default() += 1;
        }
        let filters = json!({
            "status": status, "staleness": staleness, "power": power,
            "owner_id": owner, "id_prefix": prefix,
        });
        let text = fleet_table(&target.display(), &devices, all.len());
        Ok(ToolOutput {
            structured: json!({
                "endpoint": target.display(),
                "matched": devices.len(),
                "total": all.len(),
                "filters": filters,
                "by_staleness": by_staleness,
                "by_power": by_power,
                "devices": devices,
            }),
            text: Some(text),
        })
    }

    async fn device_status(&self, args: Map<String, Value>) -> Result<ToolOutput, ToolError> {
        let target = self.target(&args)?;
        let device_id = required_device_id(&args)?;
        let include_uplinks = match args.get("uplinks") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("`uplinks` must be a boolean".into()),
        };
        let encoded = percent_encode(&device_id);
        let device_route = routes::DEVICE_PATTERN.replace("{device_id}", &encoded);
        let device = match self.get_json(&target, &device_route).await {
            Ok((_, v)) => v,
            Err(ToolError(e)) if e.contains("HTTP 404") => {
                return Err(format!(
                    "device '{device_id}' is not registered on {}; supervictor_fleet_status \
                     lists the registered ids, and ./Quickstart onboard <device> <owner> <host> \
                     registers a new one",
                    target.display()
                )
                .into());
            }
            Err(e) => return Err(e),
        };
        let (_, fleet) = self.get_json(&target, routes::FLEET).await?;
        let health = fleet
            .as_array()
            .and_then(|rows| rows.iter().find(|d| field(d, "device_id") == device_id))
            .cloned()
            .unwrap_or(Value::Null);
        let uplinks = if include_uplinks {
            let route = routes::DEVICE_UPLINKS_PATTERN.replace("{device_id}", &encoded);
            self.get_json(&target, &route).await?.1
        } else {
            Value::Null
        };
        let text = device_text(&target.display(), &device, &health, &uplinks);
        Ok(ToolOutput {
            structured: json!({
                "endpoint": target.display(),
                "device": device,
                "health": health,
                "recent_uplinks": uplinks,
            }),
            text: Some(text),
        })
    }

    async fn fleet_history(&self, args: Map<String, Value>) -> Result<ToolOutput, ToolError> {
        let target = self.target(&args)?;
        let window = optional_enum(&args, "window", WINDOWS)?.unwrap_or("1h");
        let device_id = match args.get("device_id") {
            None | Some(Value::Null) => None,
            Some(_) => Some(required_device_id(&args)?),
        };
        let summary_only = match args.get("summary_only") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("`summary_only` must be a boolean".into()),
        };
        let route = format!("{}?window={window}", routes::FLEET_HISTORY);
        let (_, mut history) = self.get_json(&target, &route).await?;
        if let Some(devices) = history.get_mut("devices").and_then(Value::as_array_mut) {
            if let Some(id) = &device_id {
                devices.retain(|d| field(d, "device_id") == id);
                // The endpoint emits a series for every registered device,
                // empty or not; an empty one is "no telemetry" to the asker.
                let silent = devices
                    .iter()
                    .all(|d| d.get("uplinks").and_then(Value::as_u64).unwrap_or(0) == 0);
                if silent {
                    return Err(format!(
                        "no telemetry from '{id}' on {} in the last {window}; the device may \
                         be unregistered, dark, or named differently (supervictor_fleet_status)",
                        target.display()
                    )
                    .into());
                }
            }
            if summary_only {
                for d in devices.iter_mut() {
                    if let Some(obj) = d.as_object_mut() {
                        obj.remove("points");
                    }
                }
            }
        }
        let text = history_text(&target.display(), &history);
        Ok(ToolOutput {
            structured: json!({ "endpoint": target.display(), "history": history }),
            text: Some(text),
        })
    }
}

impl Tools for FleetTools {
    fn definitions(&self) -> Vec<Value> {
        definitions()
    }

    fn call<'a>(&'a self, name: &'a str, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args = args.as_object().cloned().unwrap_or_default();
            match name {
                "supervictor_backend_status" => self.backend_status(args).await,
                "supervictor_fleet_summary" => self.fleet_summary(args).await,
                "supervictor_fleet_status" => self.fleet_status(args).await,
                "supervictor_device_status" => self.device_status(args).await,
                "supervictor_fleet_history" => self.fleet_history(args).await,
                other => Err(format!("unknown tool: {other}").into()),
            }
        })
    }
}

/// Tool definitions as `tools/list` returns them.
pub fn definitions() -> Vec<Value> {
    let endpoint_prop = json!({
        "type": "string",
        "description": "Endpoint URL, https://host[:port][/prefix]. Defaults to SUPERVICTOR_URL (a local ./Quickstart endpoint is https://localhost:8443). The same admin certificate is presented wherever this points."
    });
    let read_only = json!({
        "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false
    });
    vec![
        json!({
            "name": "supervictor_backend_status",
            "title": "Backend status",
            "description": "Is the supervictor endpoint (backend/server) up and is this admin identity accepted? Probes GET /health and GET /fleet/summary over mTLS and reports reachability, health, negotiated TLS version, latency, whether the admin certificate was authorized, and the fleet headline counts. An unreachable endpoint is reported as reachable=false with the reason, not as an error. Use first when anything else fails.",
            "inputSchema": {
                "type": "object",
                "properties": { "endpoint": endpoint_prop },
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
        json!({
            "name": "supervictor_fleet_summary",
            "title": "Fleet summary",
            "description": "Aggregate counts for the whole fleet of edge devices: total, active, inactive, and for active devices how many are fresh / stale / dark / never-heard and how many are powered on / off. One call, no device list; use supervictor_fleet_status for per-device rows.",
            "inputSchema": {
                "type": "object",
                "properties": { "endpoint": endpoint_prop },
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
        json!({
            "name": "supervictor_fleet_status",
            "title": "Fleet status (per device)",
            "description": "Status of every edge device (board) the endpoint knows: device_id, owner_id, lifecycle status, last_uplink time, staleness (fresh <15 min, stale <2 h, dark beyond, unknown = never heard), last current in mA, power on/off/unknown, the mA threshold it is judged against, and the firmware version it last reported. Sorted worst first (dark, stale, unknown, fresh). Optional filters narrow the list server-side of the model; counts by staleness and power are included.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "endpoint": endpoint_prop,
                    "status": { "type": "string", "enum": STATUSES, "description": "Only devices with this lifecycle status." },
                    "staleness": { "type": "string", "enum": STALENESS, "description": "Only devices in this freshness class." },
                    "power": { "type": "string", "enum": POWER, "description": "Only devices in this power state." },
                    "owner_id": { "type": "string", "description": "Only devices owned by this owner_id (exact match)." },
                    "id_prefix": { "type": "string", "description": "Only device_ids starting with this prefix, e.g. \"poe2-rack-\"." }
                },
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
        json!({
            "name": "supervictor_device_status",
            "title": "Device status",
            "description": "Everything about one edge device: its registration record (owner, certificate subject, status, registered time, threshold and chart style), its current health row (staleness, power, last uplink, firmware), and by default its ten most recent uplink payloads (current, seq, uptime_s, heap_free, temp_c, rssi_dbm, …) so reboots, gaps and sensor values can be read directly. Errors clearly when the device_id is not registered.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "device_id": { "type": "string", "minLength": 1, "maxLength": MAX_DEVICE_ID_BYTES, "description": "The device identifier (the firmware's DEVICE_NAME / the certificate CN)." },
                    "uplinks": { "type": "boolean", "default": true, "description": "Include the ten most recent uplink payloads." },
                    "endpoint": endpoint_prop
                },
                "required": ["device_id"],
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
        json!({
            "name": "supervictor_fleet_history",
            "title": "Fleet history",
            "description": "Time-bucketed telemetry over a window: a fleet staleness timeline (fresh/stale/dark/never per bucket), a power timeline (on/off/unknown per bucket), and per-device series of bucket means for current, temp_c, humidity_pct, rssi_dbm and heap_free. Windows: 1m, 5m, 15m, 1h (default), 6h, 24h, 7d. Use device_id to keep one device's series, summary_only to drop the per-bucket points and keep just the timelines and per-device uplink counts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": { "type": "string", "enum": WINDOWS, "default": "1h", "description": "How far back to look." },
                    "device_id": { "type": "string", "minLength": 1, "maxLength": MAX_DEVICE_ID_BYTES, "description": "Keep only this device's series." },
                    "summary_only": { "type": "boolean", "default": false, "description": "Omit per-bucket points from the device series." },
                    "endpoint": endpoint_prop
                },
                "additionalProperties": false
            },
            "annotations": read_only,
        }),
    ]
}

// ── argument helpers ─────────────────────────────────────────────────

fn optional_enum<'a>(
    args: &Map<String, Value>,
    key: &str,
    allowed: &'a [&'a str],
) -> Result<Option<&'a str>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => allowed
            .iter()
            .find(|a| **a == s.as_str())
            .map(|a| Some(*a))
            .ok_or_else(|| {
                ToolError(format!(
                    "`{key}` must be one of {}; got '{}'",
                    allowed.join(", "),
                    excerpt(s.as_bytes())
                ))
            }),
        Some(_) => Err(format!("`{key}` must be a string").into()),
    }
}

fn optional_string(args: &Map<String, Value>, key: &str) -> Result<Option<String>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("`{key}` must be a string").into()),
    }
}

/// `device_id`: present, non-empty, within the firmware's 64-byte contract,
/// no control characters (the endpoint rejects those on ingest too).
fn required_device_id(args: &Map<String, Value>) -> Result<String, ToolError> {
    let Some(Value::String(id)) = args.get("device_id") else {
        return Err("`device_id` (string) is required".into());
    };
    if id.is_empty() {
        return Err("`device_id` must not be empty".into());
    }
    if id.len() > MAX_DEVICE_ID_BYTES {
        return Err(format!(
            "`device_id` is {} bytes; device identifiers are at most {MAX_DEVICE_ID_BYTES}",
            id.len()
        )
        .into());
    }
    if id.chars().any(char::is_control) {
        return Err("`device_id` contains control characters".into());
    }
    Ok(id.clone())
}

/// RFC 3986 path-segment encoding: unreserved bytes pass, everything else
/// (including `/`, `?`, `#`, and spaces) becomes `%XX`, so an identifier
/// can never change the route it is asked about.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── shaping ──────────────────────────────────────────────────────────

fn field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn severity(d: &Value) -> usize {
    STALENESS
        .iter()
        .position(|s| *s == field(d, "staleness"))
        .unwrap_or(STALENESS.len())
}

fn status_output(endpoint: &str, status: Value) -> ToolOutput {
    let text = if status["reachable"] == false {
        format!(
            "{endpoint}: UNREACHABLE — {}",
            status["error"].as_str().unwrap_or("unknown error")
        )
    } else {
        let admin = if status["admin"]["authorized"] == true {
            "admin certificate accepted".to_string()
        } else {
            format!(
                "admin certificate NOT accepted ({})",
                status["admin"]["detail"]
                    .as_str()
                    .or_else(|| status["admin"]["http_status"]
                        .as_u64()
                        .map(|_| "see http_status"))
                    .unwrap_or("")
            )
        };
        let mut line = format!(
            "{endpoint}: {} (HTTP {}, {}, {} ms); {admin}",
            if status["healthy"] == true {
                "healthy"
            } else {
                "UNHEALTHY"
            },
            status["health"]["http_status"],
            status["tls_version"].as_str().unwrap_or("?"),
            status["latency_ms"],
        );
        if let Some(fleet) = status.get("fleet") {
            line.push('\n');
            line.push_str(&summary_line(fleet));
        }
        line
    };
    ToolOutput {
        structured: status,
        text: Some(text),
    }
}

fn summary_line(s: &Value) -> String {
    let n = |k: &str| s.get(k).and_then(Value::as_u64).unwrap_or(0);
    format!(
        "fleet: {} devices, {} active ({} fresh, {} stale, {} dark, {} never heard), {} inactive; power: {} on, {} off",
        n("total"), n("active"), n("fresh"), n("stale"), n("dark"), n("never"), n("inactive"), n("on"), n("off")
    )
}

fn fleet_table(endpoint: &str, devices: &[&Value], total: usize) -> String {
    let mut out = format!(
        "{endpoint}: {} of {total} devices shown\n{:<28} {:<9} {:<8} {:<8} {:>9} {:<10} {}\n",
        devices.len(),
        "DEVICE",
        "STATUS",
        "FRESH",
        "POWER",
        "CURRENT",
        "FW",
        "LAST UPLINK"
    );
    for d in devices {
        let current = d
            .get("current")
            .and_then(Value::as_i64)
            .map(|c| format!("{c} mA"))
            .unwrap_or_else(|| "-".into());
        out.push_str(&format!(
            "{:<28} {:<9} {:<8} {:<8} {:>9} {:<10} {}\n",
            truncate(field(d, "device_id"), 28),
            field(d, "status"),
            field(d, "staleness"),
            field(d, "power"),
            current,
            truncate(d.get("fw").and_then(Value::as_str).unwrap_or("-"), 10),
            d.get("last_uplink")
                .and_then(Value::as_str)
                .unwrap_or("never"),
        ));
    }
    out
}

fn device_text(endpoint: &str, device: &Value, health: &Value, uplinks: &Value) -> String {
    let mut out = format!(
        "{endpoint}: {} — owner {}, status {}, registered {}, cert {}\n",
        field(device, "device_id"),
        field(device, "owner_id"),
        field(device, "status"),
        field(device, "created_at"),
        device
            .get("subject_dn")
            .and_then(Value::as_str)
            .map(|_| "bound")
            .unwrap_or("UNBOUND (no subject_dn)"),
    );
    if health.is_object() {
        out.push_str(&format!(
            "health: {} / power {} (threshold {} mA{}), last uplink {}, fw {}\n",
            field(health, "staleness"),
            field(health, "power"),
            health
                .get("threshold_ma")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            if health.get("threshold_overridden") == Some(&Value::Bool(true)) {
                ", per-device"
            } else {
                ""
            },
            health
                .get("last_uplink")
                .and_then(Value::as_str)
                .unwrap_or("never"),
            health.get("fw").and_then(Value::as_str).unwrap_or("-"),
        ));
    } else {
        out.push_str("health: no fleet row (device absent from /fleet)\n");
    }
    if let Some(rows) = uplinks.as_array() {
        out.push_str(&format!("recent uplinks: {}\n", rows.len()));
        for u in rows.iter().take(10) {
            let p = &u["payload"];
            out.push_str(&format!(
                "  {}  current {}  seq {}  uptime_s {}  heap_free {}\n",
                field(u, "received_at"),
                p.get("current")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "-".into()),
                p.get("seq")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "-".into()),
                p.get("uptime_s")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "-".into()),
                p.get("heap_free")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "-".into()),
            ));
        }
    }
    out
}

fn history_text(endpoint: &str, h: &Value) -> String {
    let devices = h.get("devices").and_then(Value::as_array);
    let mut out = format!(
        "{endpoint}: window {} ({} buckets of {} s, {} → {}){}\n",
        field(h, "window"),
        h.get("buckets").and_then(Value::as_u64).unwrap_or(0),
        h.get("step_s").and_then(Value::as_u64).unwrap_or(0),
        field(h, "since"),
        field(h, "until"),
        if h.get("truncated") == Some(&Value::Bool(true)) {
            " — TRUNCATED at the row cap"
        } else {
            ""
        },
    );
    if let Some(last) = h
        .get("timeline")
        .and_then(Value::as_array)
        .and_then(|t| t.last())
    {
        out.push_str(&format!(
            "latest bucket: {} fresh, {} stale, {} dark, {} never\n",
            last["fresh"], last["stale"], last["dark"], last["never"]
        ));
    }
    if let Some(devices) = devices {
        for d in devices {
            out.push_str(&format!(
                "  {}: {} uplinks\n",
                field(d, "device_id"),
                d.get("uplinks").and_then(Value::as_u64).unwrap_or(0)
            ));
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_are_well_formed() {
        let defs = definitions();
        assert_eq!(defs.len(), 5);
        for d in &defs {
            let name = d["name"].as_str().unwrap();
            assert!(name.starts_with("supervictor_"), "{name}");
            assert!(d["description"].as_str().unwrap().len() > 40, "{name}");
            assert_eq!(d["inputSchema"]["type"], "object", "{name}");
            assert_eq!(d["inputSchema"]["additionalProperties"], false, "{name}");
            assert_eq!(d["annotations"]["readOnlyHint"], true, "{name}");
            assert_eq!(d["annotations"]["destructiveHint"], false, "{name}");
        }
        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "duplicate tool names");
    }

    #[test]
    fn enum_validation_names_choices() {
        let mut args = Map::new();
        args.insert("window".into(), json!("2h"));
        let err = optional_enum(&args, "window", WINDOWS).unwrap_err();
        assert!(
            err.0.contains("1m, 5m") && err.0.contains("2h"),
            "{}",
            err.0
        );
        args.insert("window".into(), json!(7));
        assert!(optional_enum(&args, "window", WINDOWS).is_err());
        args.insert("window".into(), json!("7d"));
        assert_eq!(optional_enum(&args, "window", WINDOWS).unwrap(), Some("7d"));
        args.insert("window".into(), Value::Null);
        assert_eq!(optional_enum(&args, "window", WINDOWS).unwrap(), None);
        // A hostile value is excerpted, never echoed with control chars.
        args.insert("window".into(), json!("x\n\u{1b}[2Jy"));
        let err = optional_enum(&args, "window", WINDOWS).unwrap_err();
        assert!(
            !err.0.contains('\n') && !err.0.contains('\u{1b}'),
            "{:?}",
            err.0
        );
    }

    #[test]
    fn device_id_contract() {
        let mut args = Map::new();
        assert!(required_device_id(&args).is_err());
        args.insert("device_id".into(), json!(""));
        assert!(required_device_id(&args).is_err());
        args.insert("device_id".into(), json!("a".repeat(65)));
        assert!(required_device_id(&args).unwrap_err().0.contains("64"));
        args.insert("device_id".into(), json!("a".repeat(64)));
        assert!(required_device_id(&args).is_ok());
        args.insert("device_id".into(), json!("evil\nid"));
        assert!(required_device_id(&args).is_err());
        args.insert("device_id".into(), json!(12));
        assert!(required_device_id(&args).is_err());
        args.insert("device_id".into(), json!("poe2-rack-01"));
        assert_eq!(required_device_id(&args).unwrap(), "poe2-rack-01");
    }

    #[test]
    fn percent_encoding_pins_the_route() {
        assert_eq!(percent_encode("poe2-rack-01"), "poe2-rack-01");
        assert_eq!(percent_encode("a/b?c#d e"), "a%2Fb%3Fc%23d%20e");
        assert_eq!(percent_encode("../fleet"), "..%2Ffleet");
        assert_eq!(percent_encode("ünï"), "%C3%BCn%C3%AF");
        assert_eq!(percent_encode("x,y"), "x%2Cy");
    }

    #[test]
    fn fleet_rows_sort_worst_first_and_table_renders() {
        let rows = [
            json!({"device_id":"b","status":"active","staleness":"fresh","power":"on","current":1200,"fw":"0.1.0","last_uplink":"2026-10-25T10:00:00Z"}),
            json!({"device_id":"a","status":"active","staleness":"dark","power":"unknown"}),
            json!({"device_id":"c","status":"active","staleness":"stale","power":"off","current":0}),
            json!({"device_id":"d","status":"active","staleness":"unknown","power":"unknown"}),
        ];
        let mut refs: Vec<&Value> = rows.iter().collect();
        refs.sort_by_key(|d| (severity(d), field(d, "device_id").to_string()));
        let ids: Vec<&str> = refs.iter().map(|d| field(d, "device_id")).collect();
        assert_eq!(ids, ["a", "c", "d", "b"]);
        let table = fleet_table("https://h", &refs, 9);
        assert!(table.starts_with("https://h: 4 of 9 devices shown\n"));
        assert!(table.contains("1200 mA"));
        assert!(table.contains("never"));
        assert!(table.lines().count() == 6, "{table}");
    }

    #[test]
    fn summary_and_status_text() {
        let s = json!({"total":3,"active":2,"inactive":1,"fresh":1,"stale":0,"dark":1,"never":0,"on":1,"off":0});
        assert_eq!(
            summary_line(&s),
            "fleet: 3 devices, 2 active (1 fresh, 0 stale, 1 dark, 0 never heard), 1 inactive; power: 1 on, 0 off"
        );
        let down = status_output("https://h", json!({"reachable": false, "error": "refused"}));
        assert_eq!(down.text.unwrap(), "https://h: UNREACHABLE — refused");
        let up = status_output(
            "https://h",
            json!({"reachable": true, "healthy": true, "health": {"http_status": 200}, "tls_version": "TLSv1_3", "latency_ms": 4, "admin": {"authorized": false, "detail": "403"}, "fleet": s}),
        );
        let text = up.text.unwrap();
        assert!(
            text.contains("healthy")
                && text.contains("NOT accepted (403)")
                && text.contains("fleet: 3"),
            "{text}"
        );
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "ab…");
        assert_eq!(truncate("ünïcödé", 4), "ünï…");
    }
}
