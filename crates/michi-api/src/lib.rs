use axum::{routing::get, Router};
use michi_config::Config;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

mod root;
mod status;

pub use status::StatusResponse;

pub fn create_router(config: &Config) -> Router {
    let state = config.clone();

    Router::new()
        .route("/", get(root::root_handler))
        .route("/api/status", get(status::status_handler))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
