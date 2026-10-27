use std::convert::Infallible;

use axum::extract::{FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{Extensions, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Extract the mTLS client certificate subject DN a trusted reverse proxy
/// forwarded in `x-ssl-client-subject-dn` (the Caddy / nginx convention,
/// and what the test harness uses). The proxy in front must strip that
/// header from incoming requests; this binary cannot tell. It is consulted
/// only when the binary did not terminate TLS itself — see
/// [`client_identity`]. (The Lambda Web Adapter's `x-amzn-request-context`
/// path was retired 2026-10-27 with the serverless deployment.)
pub fn extract_client_subject(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-ssl-client-subject-dn")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// The request's authenticated client identity.
///
/// When this binary terminated TLS itself (feature `tls`, `TLS_*` set), the
/// identity is the verified peer certificate's subject carried in
/// `ConnectInfo<TlsPeer>` — and forwarded headers are ignored, whatever they
/// say: nothing upstream vouched for them. Otherwise (a reverse proxy in
/// front, plain HTTP here) it is whatever the trusted proxy forwarded, per
/// [`extract_client_subject`].
pub fn client_identity(extensions: &Extensions, headers: &HeaderMap) -> Option<String> {
    #[cfg(feature = "tls")]
    if let Some(axum::extract::ConnectInfo(peer)) =
        extensions.get::<axum::extract::ConnectInfo<crate::tls::TlsPeer>>()
    {
        return peer.subject.clone();
    }
    #[cfg(not(feature = "tls"))]
    let _ = extensions;
    extract_client_subject(headers)
}

/// True when the DN has an `OU=admin` component. Component-wise parse, not a
/// substring test — `CN=OU=admin` or `CN=evil,OU=admins` must not pass.
pub fn is_admin_subject(dn: &str) -> bool {
    dn.split(',').any(|component| {
        let mut parts = component.trim().splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        key.eq_ignore_ascii_case("OU") && value.eq_ignore_ascii_case("admin")
    })
}

/// True when two subject DNs name the same certificate subject.
///
/// Component-wise: split on `,`, trim, upper-case the attribute type, sort.
/// The same certificate is rendered differently by rustls, Caddy and
/// `openssl -nameopt RFC2253` (spacing, component order), so byte equality
/// would reject legitimate devices. Anything looser than this — case-folding
/// values, collapsing escapes — risks admitting a subject the CA never
/// issued, so it is deliberately not done. An empty DN matches nothing,
/// including another empty DN: a bound device with a blank record fails
/// closed rather than open.
pub fn dn_matches(a: &str, b: &str) -> bool {
    fn canonical(dn: &str) -> Vec<(String, String)> {
        let mut parts: Vec<(String, String)> = dn
            .split(',')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(|c| {
                let (key, value) = c.split_once('=').unwrap_or((c, ""));
                (key.trim().to_ascii_uppercase(), value.trim().to_string())
            })
            .collect();
        parts.sort();
        parts
    }
    let (ca, cb) = (canonical(a), canonical(b));
    !ca.is_empty() && ca == cb
}

/// Route-layer gate: 403 unless the forwarded mTLS subject is an admin.
/// Shared by the dashboard (`/ui/*`) and the fleet JSON API (`/fleet*`).
pub async fn require_admin(request: Request, next: Next) -> Response {
    match client_identity(request.extensions(), request.headers()) {
        Some(subject) if is_admin_subject(&subject) => next.run(request).await,
        Some(subject) => {
            tracing::warn!(
                subject = %sanitize_for_log(&subject),
                "admin route denied: non-admin certificate"
            );
            forbidden("admin certificate required")
        }
        None => forbidden("client certificate required"),
    }
}

/// Plain-text 403 response.
pub fn forbidden(reason: &str) -> Response {
    (StatusCode::FORBIDDEN, format!("forbidden: {reason}")).into_response()
}

/// Strip control characters (log-injection guard) before logging
/// user-influenced strings such as cert subjects and device ids.
pub fn sanitize_for_log(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Axum extractor for mTLS client subject DN.
pub struct ClientSubject(pub Option<String>);

impl<S> FromRequestParts<S> for ClientSubject
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientSubject(client_identity(
            &parts.extensions,
            &parts.headers,
        )))
    }
}
