use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct SettingsResponse {
    pub port: u16,
    pub music_paths: Vec<String>,
    pub resource_profile: String,
    pub resource_profile_human: String,
    pub stream_profile: String,
    pub stream_profile_human: String,
    pub format_policy: String,
    pub format_policy_human: String,
    pub sync_peers: Vec<String>,
    pub sync_name: String,
    pub cors_origin: Option<String>,
    pub auth_enabled: bool,
    pub dev_mode: bool,
    pub scrobble_enabled: bool,
    pub ffmpeg_available: bool,
    pub language: String,
    pub theme: String,
    pub sidebar_collapsed: bool,
    pub cover_art_enabled: bool,
    pub auto_backup_enabled: bool,
    pub backup_max_keep: u32,
    pub job_max_concurrent: u32,
    pub reconnect_delay_max: u32,
    pub max_remote_bitrate: u32,
    pub remote_sync: bool,
}

pub async fn get_settings_handler(State(state): State<AppState>) -> Json<SettingsResponse> {
    let cfg = &state.config;
    Json(SettingsResponse {
        port: cfg.port(),
        music_paths: cfg
            .music_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        resource_profile: cfg.resource_profile.to_string(),
        resource_profile_human: cfg.human_resource_profile(),
        stream_profile: cfg.stream_profile.to_string(),
        stream_profile_human: cfg.human_stream_profile(),
        format_policy: cfg.format_policy.to_string(),
        format_policy_human: cfg.human_format_policy(),
        sync_peers: cfg.sync_peers.clone(),
        sync_name: cfg.sync_name.clone(),
        cors_origin: cfg.cors_origin.clone(),
        auth_enabled: cfg.auth_enabled,
        dev_mode: cfg.dev_mode,
        scrobble_enabled: cfg.scrobble_enabled,
        ffmpeg_available: michi_streaming::check_ffmpeg(),
        language: cfg.language.clone(),
        theme: cfg.ui.theme.clone(),
        sidebar_collapsed: cfg.ui.sidebar_collapsed,
        cover_art_enabled: cfg.ui.cover_art_enabled,
        auto_backup_enabled: cfg.auto_backup_enabled,
        backup_max_keep: cfg.backup_max_keep,
        job_max_concurrent: cfg.job_max_concurrent,
        reconnect_delay_max: cfg.reconnect_delay_max,
        max_remote_bitrate: cfg.max_remote_bitrate,
        remote_sync: cfg.remote_sync,
    })
}

#[derive(Deserialize)]
pub struct UpdateSettingsBody {
    pub resource_profile: Option<String>,
    pub stream_profile: Option<String>,
    pub format_policy: Option<String>,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub sidebar_collapsed: Option<bool>,
    pub cover_art_enabled: Option<bool>,
    pub auto_backup_enabled: Option<bool>,
    pub backup_max_keep: Option<u32>,
    pub job_max_concurrent: Option<u32>,
    pub reconnect_delay_max: Option<u32>,
    pub max_remote_bitrate: Option<u32>,
    pub remote_sync: Option<bool>,
    pub scrobble_enabled: Option<bool>,
    pub dev_mode: Option<bool>,
    pub sync_name: Option<String>,
    pub sync_peers: Option<Vec<String>>,
}

pub async fn update_settings_handler(
    State(_state): State<AppState>,
    Json(body): Json<UpdateSettingsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // We need a mutable reference to config. Since AppState stores Config immutably,
    // we persist to disk and log the changes. On next startup they'll take effect.
    // For runtime, store in a new config JSON file.

    if let Some(ref v) = body.resource_profile {
        tracing::info!("settings: resource_profile -> {}", v);
    }
    if let Some(ref v) = body.stream_profile {
        tracing::info!("settings: stream_profile -> {}", v);
    }
    if let Some(ref v) = body.format_policy {
        tracing::info!("settings: format_policy -> {}", v);
    }
    if let Some(ref v) = body.language {
        tracing::info!("settings: language -> {}", v);
    }
    if let Some(ref v) = body.theme {
        tracing::info!("settings: theme -> {}", v);
    }
    if let Some(v) = body.sidebar_collapsed {
        tracing::info!("settings: sidebar_collapsed -> {}", v);
    }
    if let Some(v) = body.cover_art_enabled {
        tracing::info!("settings: cover_art_enabled -> {}", v);
    }
    if let Some(v) = body.auto_backup_enabled {
        tracing::info!("settings: auto_backup_enabled -> {}", v);
    }
    if let Some(v) = body.backup_max_keep {
        tracing::info!("settings: backup_max_keep -> {}", v);
    }
    if let Some(v) = body.job_max_concurrent {
        tracing::info!("settings: job_max_concurrent -> {}", v);
    }
    if let Some(v) = body.reconnect_delay_max {
        tracing::info!("settings: reconnect_delay_max -> {}", v);
    }
    if let Some(v) = body.max_remote_bitrate {
        tracing::info!("settings: max_remote_bitrate -> {}", v);
    }
    if let Some(v) = body.remote_sync {
        tracing::info!("settings: remote_sync -> {}", v);
    }
    if let Some(v) = body.scrobble_enabled {
        tracing::info!("settings: scrobble_enabled -> {}", v);
    }
    if let Some(v) = body.dev_mode {
        tracing::info!("settings: dev_mode -> {}", v);
    }

    // Build a fresh config from env + current state + body overrides, persist
    let mut cfg = michi_config::Config::from_env();
    if let Some(ref v) = body.resource_profile {
        cfg.resource_profile = michi_core::ResourceProfile::from_config_str(v);
    }
    if let Some(ref v) = body.stream_profile {
        cfg.stream_profile = michi_core::StreamProfile::from_config_str(v);
    }
    if let Some(ref v) = body.format_policy {
        cfg.format_policy = michi_core::AudioFormatPolicy::from_config_str(v);
    }
    if let Some(ref v) = body.language {
        cfg.language = v.clone();
    }
    if let Some(ref v) = body.theme {
        cfg.ui.theme = v.clone();
    }
    if let Some(v) = body.sidebar_collapsed {
        cfg.ui.sidebar_collapsed = v;
    }
    if let Some(v) = body.cover_art_enabled {
        cfg.ui.cover_art_enabled = v;
    }
    if let Some(v) = body.auto_backup_enabled {
        cfg.auto_backup_enabled = v;
    }
    if let Some(v) = body.backup_max_keep {
        cfg.backup_max_keep = v;
    }
    if let Some(v) = body.job_max_concurrent {
        cfg.job_max_concurrent = v;
    }
    if let Some(v) = body.reconnect_delay_max {
        cfg.reconnect_delay_max = v;
    }
    if let Some(v) = body.max_remote_bitrate {
        cfg.max_remote_bitrate = v;
    }
    if let Some(v) = body.remote_sync {
        cfg.remote_sync = v;
    }
    if let Some(v) = body.scrobble_enabled {
        cfg.scrobble_enabled = v;
    }
    if let Some(v) = body.dev_mode {
        cfg.dev_mode = v;
    }
    if let Some(ref v) = body.sync_name {
        cfg.sync_name = v.clone();
    }
    if let Some(ref v) = body.sync_peers {
        cfg.sync_peers = v.clone();
    }

    cfg.save_to_file().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": {"code": "SAVE_ERROR", "message": e}})),
        )
    })?;

    // Restart required for all settings except UI (which take effect on next page load)
    let restart_required = body.resource_profile.is_some()
        || body.stream_profile.is_some()
        || body.format_policy.is_some()
        || body.auto_backup_enabled.is_some()
        || body.backup_max_keep.is_some()
        || body.job_max_concurrent.is_some()
        || body.reconnect_delay_max.is_some()
        || body.max_remote_bitrate.is_some()
        || body.remote_sync.is_some()
        || body.scrobble_enabled.is_some()
        || body.dev_mode.is_some()
        || body.sync_name.is_some()
        || body.sync_peers.is_some();

    Ok(Json(serde_json::json!({
        "status": "settings_updated",
        "restart_required": restart_required,
        "note": if restart_required { serde_json::Value::String("Some settings require a restart to take effect".into()) } else { serde_json::Value::Null }
    })))
}
