//! Embedded static assets for the dashboard. `include_str!`-compiled into the
//! binary so deployments stay a single artifact (no static-file serving).

use axum::http::header;
use axum::response::IntoResponse;

/// Dashboard stylesheet: the theme tokens (`themes.css`, six worlds plus
/// the OS fallback) ahead of the layout rules (`style.css`), served as one
/// file so a theme switch never waits on a second request.
pub const STYLESHEET: &str = concat!(include_str!("themes.css"), "\n", include_str!("style.css"));

/// `GET /ui/assets/style.css`
pub async fn stylesheet() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "max-age=300"),
        ],
        STYLESHEET,
    )
}
