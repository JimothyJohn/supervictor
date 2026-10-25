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
use crate::middleware::{extract_client_subject, forbidden, require_admin, sanitize_for_log};
use crate::routes::{AppState, HistoryQuery};
use crate::store::DeviceStore;
use supervictor_common::status;
use views::FleetRow;

const CSRF_COOKIE: &str = "sv_csrf";

/// Build the dashboard router. Nested by the main router when the `ui`
/// feature is enabled; every route sits behind the admin-subject gate.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ui", get(fleet).post(register_device))
        .route("/ui/history", get(history_view))
        .route("/ui/devices", axum::routing::post(register_device))
        .route("/ui/devices/{device_id}", get(device_detail))
        .route(
            "/ui/devices/{device_id}/status",
            axum::routing::post(set_status),
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
    let health = fleet::snapshot_now(store, threshold_ma)?;
    let by_id: std::collections::HashMap<String, fleet::FleetDevice> = health
        .into_iter()
        .map(|d| (d.device_id.clone(), d))
        .collect();

    Ok(devices
        .into_iter()
        .map(|device| match by_id.get(&device.device_id) {
            Some(h) => FleetRow {
                last_uplink: h.last_uplink.clone(),
                staleness: h.staleness,
                current: h.current,
                power: h.power,
                device,
            },
            None => FleetRow {
                device,
                last_uplink: None,
                staleness: fleet::Staleness::Unknown,
                current: None,
                power: fleet::PowerState::Unknown,
            },
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

/// `GET /ui/history?window=` — fleet status timeline + per-metric charts.
async fn history_view(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let window = Window::parse(query.window.as_deref())?;
    let store = state.store.as_ref();
    let now = now_unix();
    let history = history::fleet_history(store, window, now, state.power_threshold_ma)?;
    let summary = fleet::summarize(&fleet::snapshot(store, now, state.power_threshold_ma)?);
    let page = views::history_page(&history, &summary, window, &subject_of(&headers));
    Ok(page.into_response())
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
    // Colour slot = position in the fleet, so the device wears the same
    // colour here as on the fleet history page.
    let mut ids: Vec<String> = state
        .store
        .list_devices()?
        .into_iter()
        .map(|d| d.device_id)
        .collect();
    ids.sort();
    let slot = ids.iter().position(|id| *id == device_id).unwrap_or(0);
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
            slot,
            power,
            current,
            threshold_ma: state.power_threshold_ma,
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
