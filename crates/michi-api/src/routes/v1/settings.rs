use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::AppState;

fn v1_error(s: StatusCode, c: &str, m: &str) -> (StatusCode, Json<serde_json::Value>) {
    (s, Json(serde_json::json!({"error":{"code":c,"message":m}})))
}

#[derive(Serialize)]
pub struct SettingsResponse {
    pub port: u16,
    pub music_paths: Vec<String>,
    pub resource_profile: String,
    pub stream_profile: String,
    pub format_policy: String,
    pub sync_peers: Vec<String>,
    pub sync_name: String,
    pub cors_origin: Option<String>,
    pub auth_enabled: bool,
    pub dev_mode: bool,
    pub scrobble_enabled: bool,
    pub ffmpeg_available: bool,
}

pub async fn get_settings_handler(
    State(state): State<AppState>,
) -> Json<SettingsResponse> {
    let cfg = &state.config;
    Json(SettingsResponse {
        port: cfg.port(),
        music_paths: cfg.music_paths.iter().map(|p| p.display().to_string()).collect(),
        resource_profile: cfg.resource_profile.to_string(),
        stream_profile: cfg.stream_profile.to_string(),
        format_policy: cfg.format_policy.to_string(),
        sync_peers: cfg.sync_peers.clone(),
        sync_name: cfg.sync_name.clone(),
        cors_origin: cfg.cors_origin.clone(),
        auth_enabled: cfg.auth_enabled,
        dev_mode: cfg.dev_mode,
        scrobble_enabled: cfg.scrobble_enabled,
        ffmpeg_available: michi_streaming::check_ffmpeg(),
    })
}

#[derive(Deserialize)]
pub struct UpdateSettingsBody {
    pub resource_profile: Option<String>,
    pub stream_profile: Option<String>,
    pub format_policy: Option<String>,
}

pub async fn update_settings_handler(
    State(state): State<AppState>,
    Json(body): Json<UpdateSettingsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(ref profile) = body.resource_profile {
        let parsed = michi_core::ResourceProfile::from_config_str(profile);
        tracing::info!("settings: resource_profile changed to {}", parsed);
    }
    if let Some(ref profile) = body.stream_profile {
        let parsed = michi_core::StreamProfile::from_config_str(profile);
        tracing::info!("settings: stream_profile changed to {}", parsed);
    }
    if let Some(ref policy) = body.format_policy {
        let parsed = michi_core::AudioFormatPolicy::from_config_str(policy);
        tracing::info!("settings: format_policy changed to {}", parsed);
    }
    Ok(Json(serde_json::json!({"status": "settings_updated"})))
}
