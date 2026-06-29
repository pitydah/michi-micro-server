use axum::{extract::State, Json};
use serde::Serialize;
use std::path::PathBuf;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct DiagnosticsReport {
    pub healthy: bool,
    pub db: DbStatus,
    pub library: LibraryStatus,
    pub token_store: TokenStoreStatus,
    pub import_staging: ImportStagingStatus,
    pub playback: PlaybackStatus,
    pub config: ConfigStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DbStatus {
    pub connected: bool,
    pub total_tracks: i64,
    pub total_playlists: i64,
    pub total_devices: i64,
}

#[derive(Debug, Serialize)]
pub struct LibraryStatus {
    pub configured_paths: Vec<String>,
    pub paths_exist: Vec<bool>,
    pub total_tracks: i64,
}

#[derive(Debug, Serialize)]
pub struct TokenStoreStatus {
    pub active_tokens: usize,
    pub cleanup_active: bool,
}

#[derive(Debug, Serialize)]
pub struct ImportStagingStatus {
    pub staging_path: Option<String>,
    pub exists: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct PlaybackStatus {
    pub track_id: Option<String>,
    pub playing: bool,
    pub position_ms: u64,
    pub volume: u32,
}

#[derive(Debug, Serialize)]
pub struct ConfigStatus {
    pub port: u16,
    pub music_paths: Vec<String>,
    pub config_path: String,
    pub cache_path: String,
    pub database_url: String,
    pub auth_enabled: bool,
    pub dev_mode: bool,
    pub server_id: String,
}

pub async fn diagnostics_handler(
    State(state): State<AppState>,
) -> Json<DiagnosticsReport> {
    let mut warnings: Vec<String> = Vec::new();

    // Library stats
    let (total_tracks, total_albums, total_artists) = match michi_db::library_stats(&state.db).await {
        Ok(s) => (s.tracks, s.albums, s.artists),
        Err(e) => {
            warnings.push(format!("library_stats failed: {}", e));
            (0, 0, 0)
        }
    };

    // Device count
    let total_devices = michi_db::list_link_devices(&state.db).await
        .map(|d| d.len() as i64)
        .unwrap_or(0);

    // Playlist count
    let total_playlists = michi_db::list_playlists(&state.db, None).await
        .map(|p| p.len() as i64)
        .unwrap_or(0);

    // Paths
    let configured_paths: Vec<String> = state.config.music_paths.iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let paths_exist: Vec<bool> = state.config.music_paths.iter()
        .map(|p| p.exists())
        .collect();

    // Import staging
    let staging_path = state.config.music_paths.first()
        .map(|p| p.join(".import"))
        .map(|p| p.to_string_lossy().to_string());
    let staging_exists = staging_path.as_ref().map(|p| std::path::Path::new(p).exists()).unwrap_or(false);
    let staging_size = if staging_exists {
        dir_size(std::path::Path::new(staging_path.as_ref().unwrap())).unwrap_or(0)
    } else {
        0
    };

    // Token store
    let active_tokens = 0; // TokenStore is in-memory, no counter exposed

    // Playback
    let playback = state.playback_state.read().await;

    // Warnings
    if !state.config.music_paths.iter().any(|p| p.exists()) {
        warnings.push("no music paths exist on disk".into());
    }
    if state.config.auth_enabled && state.config.auth_username.is_none() {
        warnings.push("auth enabled but no username configured".into());
    }

    let report = DiagnosticsReport {
        healthy: warnings.is_empty(),
        db: DbStatus {
            connected: !state.db.is_closed(),
            total_tracks,
            total_playlists,
            total_devices,
        },
        library: LibraryStatus {
            configured_paths,
            paths_exist,
            total_tracks,
        },
        token_store: TokenStoreStatus {
            active_tokens,
            cleanup_active: true,
        },
        import_staging: ImportStagingStatus {
            staging_path,
            exists: staging_exists,
            size_bytes: staging_size,
        },
        playback: PlaybackStatus {
            track_id: playback.track_id.map(|i| i.to_string()),
            playing: playback.playing,
            position_ms: playback.position_ms,
            volume: (playback.volume * 100.0) as u32,
        },
        config: ConfigStatus {
            port: state.config.port(),
            music_paths: state.config.music_paths.iter().map(|p| p.to_string_lossy().to_string()).collect(),
            config_path: state.config.config_path.to_string_lossy().to_string(),
            cache_path: state.config.cache_path.to_string_lossy().to_string(),
            database_url: state.config.database_url.clone(),
            auth_enabled: state.config.auth_enabled,
            dev_mode: state.config.dev_mode,
            server_id: state.config.server_id.to_string(),
        },
        warnings,
    };

    Json(report)
}

fn dir_size(path: &std::path::Path) -> Option<u64> {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    total += dir_size(&entry.path()).unwrap_or(0);
                }
            }
        }
    }
    Some(total)
}
