use axum::{extract::State, Json};
use serde::Serialize;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct V1ServerInfo {
    pub service: String,
    pub name: String,
    pub server_id: Uuid,
    pub version: String,
    pub api_version: String,
    pub roles: Vec<String>,
    pub features: V1Features,
}

#[derive(Debug, Serialize)]
pub struct V1Features {
    pub library: bool,
    pub search: bool,
    pub streaming: bool,
    pub download: bool,
    pub artwork: bool,
    pub playlists: bool,
    pub sync_manifest: bool,
    pub import: bool,
    pub playback: bool,
    pub queue: bool,
    pub receivers: bool,
    pub rooms: bool,
    pub events: bool,
    pub transcoding: bool,
}

pub async fn server_info_handler(State(state): State<AppState>) -> Json<V1ServerInfo> {
    Json(V1ServerInfo {
        service: "michi-micro-server".into(),
        name: "Michi Micro Server".into(),
        server_id: state.server_id(),
        version: state.config.version().to_string(),
        api_version: "v1".into(),
        roles: vec![
            "library_server".into(),
            "stream_server".into(),
            "sync_source".into(),
            "home_server".into(),
            "playback_host".into(),
            "multiroom_host".into(),
        ],
        features: V1Features {
            library: true,
            search: true,
            streaming: true,
            download: true,
            artwork: true,
            playlists: true,
            sync_manifest: true,
            import: true,
            playback: true,
            queue: true,
            receivers: true,
            rooms: true,
            events: true,
            transcoding: false,
        },
    })
}

pub async fn status_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let uptime = state.started_at.elapsed().as_secs();
    Json(serde_json::json!({
        "status": "ok",
        "service": "michi-micro-server",
        "version": state.config.version(),
        "port": state.config.port(),
        "server_id": state.server_id(),
        "uptime_seconds": uptime,
    }))
}
