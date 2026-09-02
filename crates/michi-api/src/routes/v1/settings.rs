use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

fn v1_error(
    status: StatusCode,
    code: &str,
    message: &str,
    field: Option<&str>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut details = serde_json::Map::new();
    if let Some(f) = field {
        details.insert(
            "field".to_string(),
            serde_json::Value::String(f.to_string()),
        );
    }
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
                "details": details
            }
        })),
    )
}

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
    pub effective_scan_workers: usize,
    pub effective_transcode_workers: usize,
    pub effective_db_pool: u32,
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
    pub active: serde_json::Value,
    pub configured: serde_json::Value,
    pub restart_required: bool,
    pub pending_restart_fields: Vec<String>,
    pub effective_sources: std::collections::HashMap<String, String>,
    pub env_overrides: Vec<String>,
}

pub async fn get_settings_handler(State(state): State<AppState>) -> Json<SettingsResponse> {
    let cfg = &state.config;
    let disk_cfg = cfg.read_file_config();
    let env_overrides_set = michi_config::Config::get_env_overrides();

    let all_fields = [
        "resource_profile",
        "stream_profile",
        "format_policy",
        "auto_backup_enabled",
        "backup_max_keep",
        "job_max_concurrent",
        "reconnect_delay_max",
        "max_remote_bitrate",
        "remote_sync",
        "scrobble_enabled",
        "dev_mode",
        "sync_name",
        "sync_peers",
    ];

    let mut effective_sources = std::collections::HashMap::new();
    for f in all_fields {
        if env_overrides_set.contains(f) {
            effective_sources.insert(f.to_string(), "environment".to_string());
        } else if disk_cfg.is_some() {
            effective_sources.insert(f.to_string(), "file".to_string());
        } else {
            effective_sources.insert(f.to_string(), "default".to_string());
        }
    }

    let mut pending_restart_fields = Vec::new();
    if let Some(ref d) = disk_cfg {
        if d.resource_profile != cfg.resource_profile
            && !env_overrides_set.contains("resource_profile")
        {
            pending_restart_fields.push("resource_profile".to_string());
        }
        if d.stream_profile != cfg.stream_profile && !env_overrides_set.contains("stream_profile") {
            pending_restart_fields.push("stream_profile".to_string());
        }
        if d.format_policy != cfg.format_policy && !env_overrides_set.contains("format_policy") {
            pending_restart_fields.push("format_policy".to_string());
        }
        if d.auto_backup_enabled != cfg.auto_backup_enabled
            && !env_overrides_set.contains("auto_backup_enabled")
        {
            pending_restart_fields.push("auto_backup_enabled".to_string());
        }
        if d.backup_max_keep != cfg.backup_max_keep
            && !env_overrides_set.contains("backup_max_keep")
        {
            pending_restart_fields.push("backup_max_keep".to_string());
        }
        if d.job_max_concurrent != cfg.job_max_concurrent
            && !env_overrides_set.contains("job_max_concurrent")
        {
            pending_restart_fields.push("job_max_concurrent".to_string());
        }
        if d.reconnect_delay_max != cfg.reconnect_delay_max
            && !env_overrides_set.contains("reconnect_delay_max")
        {
            pending_restart_fields.push("reconnect_delay_max".to_string());
        }
        if d.max_remote_bitrate != cfg.max_remote_bitrate
            && !env_overrides_set.contains("max_remote_bitrate")
        {
            pending_restart_fields.push("max_remote_bitrate".to_string());
        }
        if d.remote_sync != cfg.remote_sync && !env_overrides_set.contains("remote_sync") {
            pending_restart_fields.push("remote_sync".to_string());
        }
        if d.scrobble_enabled != cfg.scrobble_enabled
            && !env_overrides_set.contains("scrobble_enabled")
        {
            pending_restart_fields.push("scrobble_enabled".to_string());
        }
        if d.dev_mode != cfg.dev_mode && !env_overrides_set.contains("dev_mode") {
            pending_restart_fields.push("dev_mode".to_string());
        }
        if d.sync_name != cfg.sync_name && !env_overrides_set.contains("sync_name") {
            pending_restart_fields.push("sync_name".to_string());
        }
        if d.sync_peers != cfg.sync_peers && !env_overrides_set.contains("sync_peers") {
            pending_restart_fields.push("sync_peers".to_string());
        }
    }

    let restart_required = !pending_restart_fields.is_empty();

    let active_val = serde_json::json!({
        "resource_profile": cfg.resource_profile.to_string(),
        "stream_profile": cfg.stream_profile.to_string(),
        "format_policy": cfg.format_policy.to_string(),
        "language": cfg.language,
        "theme": cfg.ui.theme,
        "sidebar_collapsed": cfg.ui.sidebar_collapsed,
        "cover_art_enabled": cfg.ui.cover_art_enabled,
        "effective_scan_workers": cfg.resource_profile.scan_concurrency(),
        "effective_transcode_workers": cfg.resource_profile.max_transcodes(),
        "effective_db_pool": cfg.resource_profile.db_pool_size(),
        "auto_backup_enabled": cfg.auto_backup_enabled,
        "backup_max_keep": cfg.backup_max_keep,
        "job_max_concurrent": cfg.job_max_concurrent,
        "reconnect_delay_max": cfg.reconnect_delay_max,
        "max_remote_bitrate": cfg.max_remote_bitrate,
        "remote_sync": cfg.remote_sync,
        "scrobble_enabled": cfg.scrobble_enabled,
        "dev_mode": cfg.dev_mode,
        "sync_name": cfg.sync_name,
        "sync_peers": cfg.sync_peers,
    });

    let configured_val = if let Some(ref d) = disk_cfg {
        serde_json::json!({
            "resource_profile": d.resource_profile.to_string(),
            "stream_profile": d.stream_profile.to_string(),
            "format_policy": d.format_policy.to_string(),
            "language": d.language,
            "theme": d.ui.theme,
            "sidebar_collapsed": d.ui.sidebar_collapsed,
            "cover_art_enabled": d.ui.cover_art_enabled,
            "effective_scan_workers": d.resource_profile.scan_concurrency(),
            "effective_transcode_workers": d.resource_profile.max_transcodes(),
            "effective_db_pool": d.resource_profile.db_pool_size(),
            "auto_backup_enabled": d.auto_backup_enabled,
            "backup_max_keep": d.backup_max_keep,
            "job_max_concurrent": d.job_max_concurrent,
            "reconnect_delay_max": d.reconnect_delay_max,
            "max_remote_bitrate": d.max_remote_bitrate,
            "remote_sync": d.remote_sync,
            "scrobble_enabled": d.scrobble_enabled,
            "dev_mode": d.dev_mode,
            "sync_name": d.sync_name,
            "sync_peers": d.sync_peers,
        })
    } else {
        active_val.clone()
    };

    let mut env_overrides_list: Vec<String> = env_overrides_set.into_iter().collect();
    env_overrides_list.sort();

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
        effective_scan_workers: cfg.resource_profile.scan_concurrency(),
        effective_transcode_workers: cfg.resource_profile.max_transcodes(),
        effective_db_pool: cfg.resource_profile.db_pool_size(),
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
        active: active_val,
        configured: configured_val,
        restart_required,
        pending_restart_fields,
        effective_sources,
        env_overrides: env_overrides_list,
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
    State(state): State<AppState>,
    Json(body): Json<UpdateSettingsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Strict Validation of Enums
    let parsed_resource_profile = if let Some(ref v) = body.resource_profile {
        Some(
            michi_core::ResourceProfile::from_config_str_strict(v).map_err(|e| {
                v1_error(
                    StatusCode::BAD_REQUEST,
                    "VALIDATION_ERROR",
                    e,
                    Some("resource_profile"),
                )
            })?,
        )
    } else {
        None
    };

    let parsed_stream_profile = if let Some(ref v) = body.stream_profile {
        Some(
            michi_core::StreamProfile::from_config_str_strict(v).map_err(|e| {
                v1_error(
                    StatusCode::BAD_REQUEST,
                    "VALIDATION_ERROR",
                    e,
                    Some("stream_profile"),
                )
            })?,
        )
    } else {
        None
    };

    let parsed_format_policy = if let Some(ref v) = body.format_policy {
        Some(
            michi_core::AudioFormatPolicy::from_config_str_strict(v).map_err(|e| {
                v1_error(
                    StatusCode::BAD_REQUEST,
                    "VALIDATION_ERROR",
                    e,
                    Some("format_policy"),
                )
            })?,
        )
    } else {
        None
    };

    // 2. Strict Range Validation
    if let Some(v) = body.job_max_concurrent {
        if !(1..=32).contains(&v) {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "job_max_concurrent must be between 1 and 32",
                Some("job_max_concurrent"),
            ));
        }
    }

    if let Some(v) = body.backup_max_keep {
        if !(1..=100).contains(&v) {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "backup_max_keep must be between 1 and 100",
                Some("backup_max_keep"),
            ));
        }
    }

    if let Some(v) = body.reconnect_delay_max {
        if !(5..=3600).contains(&v) {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "reconnect_delay_max must be between 5 and 3600 seconds",
                Some("reconnect_delay_max"),
            ));
        }
    }

    if let Some(v) = body.max_remote_bitrate {
        if !(32000..=20000000).contains(&v) {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "max_remote_bitrate must be between 32000 and 20000000 bps",
                Some("max_remote_bitrate"),
            ));
        }
    }

    // 3. Build a fresh config from current disk or state config + overrides, and persist
    let mut cfg = state
        .config
        .read_file_config()
        .unwrap_or_else(|| state.config.clone());

    if let Some(rp) = parsed_resource_profile {
        cfg.resource_profile = rp;
    }
    if let Some(sp) = parsed_stream_profile {
        cfg.stream_profile = sp;
    }
    if let Some(fp) = parsed_format_policy {
        cfg.format_policy = fp;
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

    // Restart required for all settings except UI preferences
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
