use axum::{
    routing::{delete, get, post},
    Router,
};
use michi_config::Config;
use sqlx::SqlitePool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

mod library;
mod root;
mod status;
mod stream;

pub use status::StatusResponse;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Config,
    pub db: SqlitePool,
}

impl AppState {
    pub fn new(config: Config, db: SqlitePool) -> Self {
        Self { config, db }
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root::root_handler))
        .route("/api/status", get(status::status_handler))
        .route("/api/library/scan", post(library::scan_handler))
        .route("/api/library/stats", get(library::stats_handler))
        .route(
            "/api/library/tracks",
            delete(library::delete_all_tracks_handler),
        )
        .route("/api/tracks", get(library::tracks_handler))
        .route(
            "/api/tracks/:id",
            get(library::track_handler)
                .delete(library::delete_track_handler)
                .put(library::update_track_handler),
        )
        .route("/api/stream/:id", get(stream::stream_handler))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
