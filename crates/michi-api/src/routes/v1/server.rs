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
    pub deployment_platform: String,
    pub commit: Option<String>,
    pub api_version: String,
    pub identity_scheme: String,
    pub michi_id: String,
    pub public_key: String,
    pub michi_home_id: String,
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
    pub sync: bool,
    pub sync_manifest: bool,
    pub import: bool,
    pub playback: bool,
    pub queue: bool,
    pub receivers: bool,
    pub rooms: bool,
    pub multiroom: bool,
    pub events: bool,
    pub transcoding: bool,
    pub token_refresh: bool,
    pub aircat_original_v1: bool,
    pub home_tls_spki_pinned: bool,
    pub aircat_manifest_v1: bool,
    pub aircat_authorize_v1: bool,
    pub aircat_original_download_v1: bool,
}

pub async fn server_info_handler(State(state): State<AppState>) -> Json<V1ServerInfo> {
    let caps = crate::server_caps::ServerCapabilities::from_state(&state).await;
    Json(V1ServerInfo {
        service: "michi-micro-server".into(),
        name: "Michi Micro Server".into(),
        server_id: state.server_id(),
        version: state.config.version().to_string(),
        deployment_platform: state.config.deployment_platform.clone(),
        commit: crate::assets::build_commit(),
        api_version: "v1".into(),
        identity_scheme: michi_identity::IDENTITY_SCHEME_V1.into(),
        michi_id: state.identity.michi_id().to_base64url(),
        public_key: state.identity.public_key_base64url(),
        michi_home_id: state.home_root_authority.home_id(),
        roles: michi_link::CANONICAL_MICRO_ROLES
            .iter()
            .map(|r| r.as_str().to_string())
            .collect(),
        features: V1Features {
            library: caps.canonical.library,
            search: caps.canonical.search,
            streaming: caps.canonical.streaming,
            download: caps.canonical.download,
            artwork: caps.canonical.artwork,
            playlists: caps.canonical.playlists,
            sync: caps.canonical.sync,
            sync_manifest: caps.canonical.sync_manifest,
            import: caps.feature_enabled("import"),
            playback: caps.canonical.playback,
            queue: caps.canonical.queue,
            receivers: caps.canonical.receivers,
            rooms: caps.feature_enabled("rooms"),
            multiroom: caps.canonical.multiroom,
            events: caps.canonical.events,
            transcoding: caps.transcoding_ready,
            token_refresh: caps.feature_enabled("token_refresh"),
            aircat_original_v1: caps.canonical.aircat_original_v1,
            home_tls_spki_pinned: caps.canonical.home_tls_spki_pinned,
            aircat_manifest_v1: caps.canonical.aircat_manifest_v1,
            aircat_authorize_v1: caps.canonical.aircat_authorize_v1,
            aircat_original_download_v1: caps.canonical.aircat_original_download_v1,
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
    let db_ok = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tracks")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let music_paths: Vec<String> = state
        .config
        .music_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    Json(serde_json::json!({
        "status": "ok",
        "service": "michi-micro-server",
        "name": "Michi Micro Server",
        "version": state.config.version(),
        "port": state.config.port(),
        "server_id": state.server_id(),
        "uptime_seconds": uptime,
        "database": if db_ok { "connected" } else { "disconnected" },
        "music_paths": music_paths,
        "resource_profile": state.config.resource_profile.to_string(),
        "stream_profile": state.config.stream_profile.to_string(),
    }))
}
