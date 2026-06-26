use axum::{extract::State, Json};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct StatusResponse {
    pub status: String,
    pub name: String,
    pub version: String,
    pub port: u16,
    pub music_paths: usize,
    pub database: String,
    pub server_id: Uuid,
    pub uptime_seconds: u64,
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
    let uptime = state.started_at.elapsed().as_secs();
    let db_status = if state.db.is_closed() { "error" } else { "ok" };
    Json(StatusResponse {
        status: "ok".to_string(),
        name: "Michi Micro Server".to_string(),
        version: state.config.version().to_string(),
        port: state.config.port(),
        music_paths: state.config.music_paths.len(),
        database: db_status.to_string(),
        server_id: state.server_id(),
        uptime_seconds: uptime,
    })
}
