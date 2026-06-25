use axum::{extract::State, Json};
use michi_config::Config;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub service: String,
    pub version: String,
    pub port: u16,
}

pub async fn status_handler(State(config): State<Config>) -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok".to_string(),
        service: "michi-micro-server".to_string(),
        version: config.version().to_string(),
        port: config.port(),
    })
}
