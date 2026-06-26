use axum::{extract::State, Json};
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct StatusResponse {
    pub status: String,
    pub service: String,
    pub version: String,
    pub port: u16,
    pub music_paths: usize,
}

#[utoipa::path(
    get,
    path = "/api/status",
    tag = "Status",
    responses(
        (status = 200, description = "Server status", body = StatusResponse)
    )
)]
pub async fn status_handler(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok".to_string(),
        service: "michi-micro-server".to_string(),
        version: state.config.version().to_string(),
        port: state.config.port(),
        music_paths: state.config.music_paths.len(),
    })
}
