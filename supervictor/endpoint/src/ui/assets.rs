//! Embedded static assets for the dashboard. `include_str!`-compiled into the
//! binary so deployments stay a single artifact (no static-file serving).

use std::sync::OnceLock;

use axum::http::header;
use axum::response::IntoResponse;

/// Dashboard stylesheet: the theme tokens (`themes.css`, six worlds plus
/// the OS fallback) ahead of the layout rules (`style.css`), served as one
/// file so a theme switch never waits on a second request.
pub const STYLESHEET: &str = concat!(include_str!("themes.css"), "\n", include_str!("style.css"));

/// The stylesheet's URL with a content hash in the query, so a new build
/// is never served against a browser's cached copy of the old one (the
/// theme switch silently did nothing once, for exactly that reason).
pub fn stylesheet_href() -> &'static str {
    static HREF: OnceLock<String> = OnceLock::new();
    HREF.get_or_init(|| {
        // FNV-1a: stable, dependency-free, plenty for a cache key.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in STYLESHEET.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("/ui/assets/style.css?v={hash:016x}")
    })
}

/// `GET /ui/assets/style.css` (the query is ignored; pages link the
/// hashed URL, so the long cache life is safe).
pub async fn stylesheet() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        STYLESHEET,
    )
}
