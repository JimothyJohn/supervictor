use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;

/// Extract mTLS client certificate subject DN from request headers.
///
/// Checks (in order):
/// 1. x-amzn-request-context header (Lambda Web Adapter / API Gateway)
/// 2. x-ssl-client-subject-dn header (nginx/Caddy reverse proxy)
/// 3. None (local dev, no mTLS)
pub fn extract_client_subject(headers: &HeaderMap) -> Option<String> {
    // Lambda Web Adapter passes API Gateway requestContext as a header
    if let Some(ctx_header) = headers.get("x-amzn-request-context") {
        if let Ok(ctx_str) = ctx_header.to_str() {
            if let Ok(ctx) = serde_json::from_str::<serde_json::Value>(ctx_str) {
                if let Some(subject) = ctx
                    .get("identity")
                    .and_then(|id| id.get("clientCert"))
                    .and_then(|cert| cert.get("subjectDN"))
                    .and_then(|s| s.as_str())
                {
                    return Some(subject.to_string());
                }
            }
        }
    }

    // Reverse proxy / ingress controller convention
    if let Some(ssl_subject) = headers.get("x-ssl-client-subject-dn") {
        if let Ok(s) = ssl_subject.to_str() {
            return Some(s.to_string());
        }
    }

    None
}

/// Axum extractor for mTLS client subject DN.
pub struct ClientSubject(pub Option<String>);

impl<S> FromRequestParts<S> for ClientSubject
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientSubject(extract_client_subject(&parts.headers)))
    }
}
