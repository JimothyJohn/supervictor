//! Fleet dashboard: server-rendered HTML behind admin mTLS.
//!
//! Every `/ui` route requires a verified client-cert subject containing an
//! `OU=admin` component (device certs carry other OUs and get 403). The proxy
//! layer (Caddy / API Gateway truststore) does the cryptographic validation;
//! this module trusts only the forwarded subject header, same as the API.

/// Embedded CSS and other static assets.
pub mod assets;
/// Inline-SVG charts for the history views.
pub mod charts;
/// Server-Sent Events uplink stream.
pub mod sse;
/// maud page templates.
pub mod views;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderMap};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crate::error::AppError;
use crate::fleet;
use crate::history::{self, Window};
use crate::live;
use crate::middleware::{extract_client_subject, forbidden, require_admin, sanitize_for_log};
use crate::routes::{AppState, HistoryQuery};
use crate::store::DeviceStore;
use crate::style;
use supervictor_common::models::{DeviceStyle, LineStyle, Marker};
use supervictor_common::status;
use views::{FleetRow, Selection};

const CSRF_COOKIE: &str = "sv_csrf";

/// Build the dashboard router. Nested by the main router when the `ui`
/// feature is enabled; every route sits behind the admin-subject gate.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ui", get(fleet).post(register_device))
        .route("/ui/history", get(history_view))
        .route("/ui/live", get(live_view))
        .route("/ui/live.json", get(live_json))
        .route("/ui/devices", axum::routing::post(register_device))
        .route("/ui/devices/{device_id}", get(device_detail))
        .route(
            "/ui/devices/{device_id}/status",
            axum::routing::post(set_status),
        )
        .route(
            "/ui/devices/{device_id}/threshold",
            axum::routing::post(set_threshold),
        )
        .route(
            "/ui/devices/{device_id}/style",
            axum::routing::post(set_style),
        )
        .route("/ui/events", get(sse::events))
        .route("/ui/assets/style.css", get(assets::stylesheet))
        .route_layer(middleware::from_fn(require_admin))
        .with_state(state)
}

// ── CSRF (double-submit cookie) ───────────────────────────────────────

/// Generate an unguessable token. Two independent `RandomState`s (SipHash
/// keys seeded from OS entropy at first use) hash a counter + clock into 128
/// bits. Not a general-purpose CSPRNG — sufficient for double-submit
/// unguessability; swap for `getrandom` if the bar ever rises.
fn csrf_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    static SEEDS: OnceLock<(RandomState, RandomState)> = OnceLock::new();
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let (a, b) = SEEDS.get_or_init(|| (RandomState::new(), RandomState::new()));
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut h1 = a.build_hasher();
    h1.write_u128(now);
    h1.write_u64(n);
    let mut h2 = b.build_hasher();
    h2.write_u128(now);
    h2.write_u64(n ^ 0x9e37_79b9_7f4a_7c15);
    format!("{:016x}{:016x}", h1.finish(), h2.finish())
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get_all(header::COOKIE).iter().find_map(|value| {
        value.to_str().ok()?.split(';').find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
    })
}

/// Existing token from the request cookie, or a fresh one plus the
/// `Set-Cookie` header that must accompany the response.
fn ensure_csrf(headers: &HeaderMap) -> (String, Option<[(header::HeaderName, String); 1]>) {
    match cookie_value(headers, CSRF_COOKIE) {
        Some(token) => (token, None),
        None => {
            let token = csrf_token();
            // No `Secure` attribute: TLS is terminated by Caddy/API GW, and
            // SAM-local dev is plain http. SameSite=Strict is the backstop.
            let cookie = format!("{CSRF_COOKIE}={token}; Path=/ui; SameSite=Strict; HttpOnly");
            (token, Some([(header::SET_COOKIE, cookie)]))
        }
    }
}

/// Constant-time-ish comparison; length mismatch short-circuits, which leaks
/// nothing useful for a random per-session token.
fn csrf_ok(headers: &HeaderMap, submitted: &str) -> bool {
    match cookie_value(headers, CSRF_COOKIE) {
        Some(cookie) if cookie.len() == submitted.len() && !cookie.is_empty() => {
            cookie
                .bytes()
                .zip(submitted.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
        }
        _ => false,
    }
}

// ── Page handlers ─────────────────────────────────────────────────────

/// Assemble fleet rows for the HTML table from a fleet snapshot.
pub fn fleet_rows(store: &dyn DeviceStore, threshold_ma: i64) -> Result<Vec<FleetRow>, AppError> {
    // Re-fetch devices for full records; snapshot carries the health fields.
    let devices = store.list_devices()?;
    let mut styles = style::resolve(&devices);
    let health = fleet::snapshot_now(store, threshold_ma)?;
    let by_id: std::collections::HashMap<String, fleet::FleetDevice> = health
        .into_iter()
        .map(|d| (d.device_id.clone(), d))
        .collect();

    Ok(devices
        .into_iter()
        .map(|device| {
            let style = styles
                .remove(&device.device_id)
                .unwrap_or_else(|| style::Resolved::slot(style::SLOTS as usize));
            match by_id.get(&device.device_id) {
                Some(h) => FleetRow {
                    last_uplink: h.last_uplink.clone(),
                    staleness: h.staleness,
                    current: h.current,
                    power: h.power,
                    threshold_ma: h.threshold_ma,
                    device,
                    style,
                },
                None => FleetRow {
                    threshold_ma: device.effective_threshold_ma(threshold_ma),
                    device,
                    last_uplink: None,
                    staleness: fleet::Staleness::Unknown,
                    current: None,
                    power: fleet::PowerState::Unknown,
                    style,
                },
            }
        })
        .collect())
}

fn subject_of(headers: &HeaderMap) -> String {
    extract_client_subject(headers).unwrap_or_else(|| "unknown".into())
}

async fn fleet(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let rows = fleet_rows(state.store.as_ref(), state.power_threshold_ma)?;
    let (token, set_cookie) = ensure_csrf(&headers);
    let page = views::fleet_page(&rows, &token, &subject_of(&headers));
    Ok(match set_cookie {
        Some(cookie) => (cookie, page).into_response(),
        None => page.into_response(),
    })
}

fn now_unix() -> u64 {
    crate::time::parse_rfc3339_unix(&crate::time::now_rfc3339()).unwrap_or(0)
}

/// The history page's query: `window=` (one preset) and any number of
/// `device=` keys naming the devices to plot. Parsed as raw pairs because
/// a device id may contain any non-control byte, commas included, so a
/// repeated key is the only unambiguous list encoding.
fn history_query(pairs: &[(String, String)]) -> Result<(Window, Vec<String>), AppError> {
    let window = pairs
        .iter()
        .rev()
        .find(|(k, _)| k == "window")
        .map(|(_, v)| v.as_str());
    let window = Window::parse(window)?;
    let mut devices: Vec<String> = Vec::new();
    for (_, v) in pairs.iter().filter(|(k, _)| k == "device") {
        if !v.is_empty() && !devices.contains(v) {
            devices.push(v.clone());
        }
    }
    Ok((window, devices))
}

/// `GET /ui/history?window=&device=&device=` — fleet status timeline,
/// power stack, and per-metric charts for the selected devices. Unknown
/// device ids in the selection are ignored (a bookmark outlives a device);
/// no selection plots no traces.
async fn history_view(
    State(state): State<AppState>,
    Query(pairs): Query<Vec<(String, String)>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (window, wanted) = history_query(&pairs)?;
    let store = state.store.as_ref();
    let now = now_unix();
    let history = history::fleet_history(store, window, now, state.power_threshold_ma)?;
    let summary = fleet::summarize(&fleet::snapshot(store, now, state.power_threshold_ma)?);
    let styles = style::resolve(&store.list_devices()?);
    // Fleet order, known ids only.
    let selected: Vec<String> = history
        .devices
        .iter()
        .filter(|d| wanted.contains(&d.device_id))
        .map(|d| d.device_id.clone())
        .collect();
    let page = views::history_page(
        &history,
        &summary,
        window,
        &subject_of(&headers),
        &Selection {
            styles: &styles,
            selected: &selected,
        },
    );
    Ok(page.into_response())
}

/// `GET /ui/live` — 8-bit register bank + scrolling strips.
async fn live_view(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let snapshot = live::snapshot(state.store.as_ref(), crate::time::now_unix_ms())?;
    Ok(views::live_page(&snapshot, &subject_of(&headers)).into_response())
}

/// `GET /ui/live.json` — the same snapshot, for the page's polling
/// fallback where SSE does not get through.
async fn live_json(State(state): State<AppState>) -> Result<axum::Json<live::Snapshot>, AppError> {
    Ok(axum::Json(live::snapshot(
        state.store.as_ref(),
        crate::time::now_unix_ms(),
    )?))
}

/// `GET /ui/devices/{id}?window=` — one device's charts + raw uplinks.
async fn device_detail(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Query(query): Query<HistoryQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let window = Window::parse(query.window.as_deref())?;
    let device = state
        .store
        .get_device(&device_id)?
        .ok_or(AppError::DeviceNotFound {
            device_id: device_id.clone(),
        })?;
    // Same resolved style as the fleet pages, so the device wears the same
    // colour everywhere.
    let style = style::resolve(&state.store.list_devices()?)
        .remove(&device_id)
        .unwrap_or_else(|| style::Resolved::slot(style::SLOTS as usize));
    let now = now_unix();
    let (series, truncated) =
        history::device_history(state.store.as_ref(), &device_id, window, now)?;
    let health = fleet::snapshot(state.store.as_ref(), now, state.power_threshold_ma)?
        .into_iter()
        .find(|d| d.device_id == device_id);
    let (power, current) = health
        .map(|h| (h.power, h.current))
        .unwrap_or((fleet::PowerState::Unknown, None));
    let uplinks = state.store.get_uplinks(&device_id, 20)?;
    let (token, set_cookie) = ensure_csrf(&headers);
    let page = views::device_page(
        &device,
        &views::DeviceHistory {
            series: &series,
            window,
            truncated,
            style,
            power,
            current,
            threshold_ma: device.effective_threshold_ma(state.power_threshold_ma),
            fleet_threshold_ma: state.power_threshold_ma,
        },
        &uplinks,
        &token,
        &subject_of(&headers),
    );
    Ok(match set_cookie {
        Some(cookie) => (cookie, page).into_response(),
        None => page.into_response(),
    })
}

// ── Fleet actions (mutating; CSRF-checked, audit-logged) ─────────────

#[derive(Deserialize)]
struct RegisterForm {
    csrf: String,
    device_id: String,
    owner_id: String,
    #[serde(default)]
    subject_dn: String,
}

async fn register_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<RegisterForm>,
) -> Result<Response, AppError> {
    if !csrf_ok(&headers, &form.csrf) {
        return Ok(forbidden("invalid CSRF token"));
    }
    // Reuse the API handler's validation by round-tripping through the same
    // JSON contract — form → RegisterDeviceRequest → handle_register_device.
    let request = serde_json::json!({
        "device_id": form.device_id,
        "owner_id": form.owner_id,
        "subject_dn": if form.subject_dn.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(form.subject_dn.clone())
        },
    });
    crate::handlers::handle_register_device(Some(&request.to_string()), state.store.as_ref())?;
    tracing::info!(
        admin = %sanitize_for_log(&subject_of(&headers)),
        device_id = %sanitize_for_log(&form.device_id),
        action = "register",
        "fleet action"
    );
    Ok(Redirect::to("/ui").into_response())
}

#[derive(Deserialize)]
struct StatusForm {
    csrf: String,
    status: String,
}

async fn set_status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<StatusForm>,
) -> Result<Response, AppError> {
    if !csrf_ok(&headers, &form.csrf) {
        return Ok(forbidden("invalid CSRF token"));
    }
    // Whitelist, never pass client input through to storage.
    let status = match form.status.as_str() {
        s if s == status::ACTIVE => status::ACTIVE,
        s if s == status::INACTIVE => status::INACTIVE,
        _ => {
            return Err(AppError::InvalidPayload {
                detail: "status must be 'active' or 'inactive'".into(),
                structured: None,
            })
        }
    };
    state.store.set_device_status(&device_id, status)?;
    tracing::info!(
        admin = %sanitize_for_log(&subject_of(&headers)),
        device_id = %sanitize_for_log(&device_id),
        action = %status,
        "fleet action"
    );
    Ok(Redirect::to("/ui").into_response())
}

#[derive(Deserialize)]
struct ThresholdForm {
    csrf: String,
    /// Threshold in mA; empty clears the override (back to the fleet default).
    #[serde(default)]
    threshold_ma: String,
}

async fn set_threshold(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ThresholdForm>,
) -> Result<Response, AppError> {
    if !csrf_ok(&headers, &form.csrf) {
        return Ok(forbidden("invalid CSRF token"));
    }
    let raw = form.threshold_ma.trim();
    let threshold = if raw.is_empty() {
        None
    } else {
        Some(raw.parse::<i64>().map_err(|_| AppError::InvalidPayload {
            detail: "threshold_ma must be a whole number of milliamps, or empty to clear".into(),
            structured: None,
        })?)
    };
    // Same validation and store path as the JSON API.
    let body = serde_json::json!({ "power_threshold_ma": threshold }).to_string();
    crate::handlers::handle_set_device_threshold(&device_id, Some(&body), state.store.as_ref())?;
    tracing::info!(
        admin = %sanitize_for_log(&subject_of(&headers)),
        device_id = %sanitize_for_log(&device_id),
        action = "threshold",
        threshold_ma = ?threshold,
        "fleet action"
    );
    Ok(Redirect::to(&format!("/ui/devices/{device_id}")).into_response())
}

#[derive(Deserialize)]
struct StyleForm {
    csrf: String,
    /// Palette slot token, `custom` (read `custom_color`), or empty for
    /// no stored colour.
    #[serde(default)]
    color: String,
    #[serde(default)]
    custom_color: String,
    #[serde(default)]
    line_style: String,
    #[serde(default)]
    marker: String,
}

async fn set_style(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<StyleForm>,
) -> Result<Response, AppError> {
    if !csrf_ok(&headers, &form.csrf) {
        return Ok(forbidden("invalid CSRF token"));
    }
    let bad = |what: &str| AppError::InvalidPayload {
        detail: format!("{what}: not an option"),
        structured: None,
    };
    let color = match form.color.trim() {
        "" => None,
        "custom" => Some(form.custom_color.trim().to_string()),
        slot => Some(slot.to_string()),
    };
    // Default options submit as their own token; `solid` / `none` store as
    // unset so the record carries only what differs from the default.
    let line_style = match form.line_style.trim() {
        "" => None,
        t => Some(LineStyle::parse(t).ok_or_else(|| bad("line_style"))?),
    }
    .filter(|l| *l != LineStyle::Solid);
    let marker = match form.marker.trim() {
        "" => None,
        t => Some(Marker::parse(t).ok_or_else(|| bad("marker"))?),
    }
    .filter(|m| *m != Marker::None);
    // Same validation and store path as the JSON API.
    let body = serde_json::to_string(&DeviceStyle {
        color,
        line_style,
        marker,
    })
    .map_err(|e| AppError::InvalidPayload {
        detail: e.to_string(),
        structured: None,
    })?;
    let saved =
        crate::handlers::handle_set_device_style(&device_id, Some(&body), state.store.as_ref())?;
    tracing::info!(
        admin = %sanitize_for_log(&subject_of(&headers)),
        device_id = %sanitize_for_log(&device_id),
        action = "style",
        color = ?saved.color,
        line_style = ?saved.line_style,
        marker = ?saved.marker,
        "fleet action"
    );
    Ok(Redirect::to(&format!("/ui/devices/{device_id}")).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(q: &[(&str, &str)]) -> Vec<(String, String)> {
        q.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn history_query_takes_repeated_device_keys_in_order_without_dupes() {
        let (w, d) = history_query(&pairs(&[])).unwrap();
        assert_eq!(w, Window::DEFAULT);
        assert!(d.is_empty());
        let (w, d) = history_query(&pairs(&[
            ("device", "b,2"),
            ("window", "5m"),
            ("device", ""),
            ("device", "a"),
            ("device", "b,2"),
            ("other", "x"),
        ]))
        .unwrap();
        assert_eq!(w, Window::M5);
        assert_eq!(d, ["b,2", "a"], "commas are id bytes, empties drop");
        assert!(history_query(&pairs(&[("window", "2m")])).is_err());
    }
}
