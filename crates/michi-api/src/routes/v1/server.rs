use axum::{extract::State, http::StatusCode, Json};
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
    pub auth: V1AuthInfo,
}

#[derive(Debug, Serialize)]
pub struct V1AuthInfo {
    pub required: bool,
    pub strategy: String,
    pub token_refresh: bool,
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
    pub token_refresh: bool,
}

pub async fn server_info_handler(State(state): State<AppState>) -> Json<V1ServerInfo> {
    let caps = crate::server_caps::ServerCapabilities::from_state(&state).await;
    Json(V1ServerInfo {
        service: "michi-micro-server".into(),
        name: "Michi Micro Server".into(),
        server_id: state.server_id(),
        version: state.config.version().to_string(),
        api_version: "v1".into(),
        roles: michi_link::CANONICAL_MICRO_ROLES
            .iter()
            .map(|r| r.as_str().to_string())
            .collect(),
        features: V1Features {
            library: caps.feature_enabled("library"),
            search: caps.feature_enabled("search"),
            streaming: caps.feature_enabled("stream"),
            download: caps.feature_enabled("download"),
            artwork: caps.feature_enabled("artwork"),
            playlists: caps.feature_enabled("playlists"),
            sync_manifest: caps.feature_enabled("sync"),
            import: caps.feature_enabled("import"),
            playback: caps.feature_enabled("playback"),
            queue: caps.feature_enabled("queue"),
            receivers: caps.feature_enabled("receivers"),
            rooms: caps.feature_enabled("rooms"),
            events: caps.feature_enabled("events"),
            transcoding: caps.feature_enabled("transcoding"),
            token_refresh: caps.feature_enabled("token_refresh"),
        },
        auth: V1AuthInfo {
            required: true,
            strategy: "SERVER_CODE".into(),
            token_refresh: true,
        },
    })
}

pub async fn health_live_handler() -> &'static str {
    "OK"
}

pub async fn health_ready_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db_ok = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tracks")
        .fetch_one(&state.db)
        .await
        .is_ok();

    if db_ok {
        Ok(Json(serde_json::json!({ "status": "ok" })))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "error", "message": "database unavailable" })),
        ))
    }
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
        "resource_profile": state.config.resource_profile.to_string(),
        "stream_profile": state.config.stream_profile.to_string(),
    }))
}
