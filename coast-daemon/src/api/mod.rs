pub mod query;
pub mod routes;
pub mod streaming;
#[cfg(test)]
mod tests;
pub mod websocket;
pub mod ws_exec;
pub mod ws_host_service_exec;
pub mod ws_host_service_logs;
pub mod ws_host_service_stats;
pub mod ws_host_terminal;
pub mod ws_logs;
pub mod ws_lsp;
pub mod ws_remote_exec;
pub mod ws_remote_logs;
pub mod ws_remote_stats;
pub mod ws_service_exec;
pub mod ws_service_stats;
pub mod ws_stats;

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use rust_embed::Embed;
use tower_http::cors::{Any, CorsLayer};

use crate::server::AppState;

pub const DEFAULT_API_PORT: u16 = 31415;

#[derive(Embed)]
#[folder = "../coast-guard/dist/"]
#[prefix = ""]
struct UiAssets;

/// Resolve a disk-based UI override directory. When `COAST_UI_DIR` is set (or
/// a local `coast-guard/dist` exists on disk), serve from there instead of the
/// embedded assets. This is useful during UI development.
fn resolve_ui_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("COAST_UI_DIR") {
        let path = PathBuf::from(dir);
        if path.join("index.html").exists() {
            return Some(path);
        }
    }
    None
}

pub fn api_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_v1 = Router::new()
        .merge(routes::router())
        .merge(query::router())
        .nest("/stream", streaming::router())
        .merge(websocket::router())
        .merge(ws_logs::router())
        .merge(ws_exec::router())
        .merge(ws_host_terminal::router())
        .merge(ws_stats::router())
        .merge(ws_service_exec::router())
        .merge(ws_service_stats::router())
        .merge(ws_host_service_exec::router())
        .merge(ws_host_service_logs::router())
        .merge(ws_host_service_stats::router())
        .merge(ws_lsp::router())
        .merge(ws_remote_exec::router())
        .merge(ws_remote_stats::router())
        .merge(ws_remote_logs::router());

    let mut router = Router::new()
        .nest("/api/v1", api_v1)
        .layer(cors)
        .with_state(state);

    if let Some(ui_dir) = resolve_ui_dir() {
        tracing::info!(path = %ui_dir.display(), "serving Coast Guard UI from disk (override)");
        let serve = tower_http::services::ServeDir::new(&ui_dir)
            .append_index_html_on_directories(true)
            .fallback(tower_http::services::ServeFile::new(
                ui_dir.join("index.html"),
            ));
        router = router.fallback_service(serve);
    } else {
        tracing::info!("serving Coast Guard UI from embedded assets");
        router = router.fallback(serve_embedded_ui);
    }

    router
}

async fn serve_embedded_ui(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try the exact path first, then fall back to index.html for SPA routing
    if let Some(content) = UiAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.to_vec()))
            .unwrap()
    } else if let Some(index) = UiAssets::get("index.html") {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(index.data.to_vec()))
            .unwrap()
    } else {
        (StatusCode::NOT_FOUND, "Coast Guard UI not available").into_response()
    }
}
