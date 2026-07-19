use axum::{extract::State, Json};
use serde::Serialize;

use crate::AppState;
// Platform-specific system metrics helpers

/// Read memory info from /proc/self/status
fn read_memory() -> (u64, u64) {
    if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
        let mut rss = 0u64;
        let mut vm = 0u64;
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                rss = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
            if line.starts_with("VmSize:") {
                vm = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }
        return (rss / 1024, vm / 1024); // Convert KB to MB
    }
    (0, 0)
}

/// Read thread count from /proc/self/stat or /proc/self/task
fn read_thread_count() -> u32 {
    if let Ok(entries) = std::fs::read_dir("/proc/self/task") {
        return entries.count() as u32;
    }
    0
}

/// Read binary size
fn read_binary_size() -> u64 {
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(meta) = std::fs::metadata(exe) {
            return meta.len();
        }
    }
    0
}

/// Read CPU usage from /proc/self/stat (utime + stime)
fn read_cpu_ticks() -> u64 {
    if let Ok(content) = std::fs::read_to_string("/proc/self/stat") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 15 {
            let utime: u64 = parts[13].parse().unwrap_or(0);
            let stime: u64 = parts[14].parse().unwrap_or(0);
            return utime + stime;
        }
    }
    0
}

fn read_total_cpu_ticks() -> u64 {
    if let Ok(content) = std::fs::read_to_string("/proc/stat") {
        for line in content.lines() {
            if line.starts_with("cpu ") {
                return line
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|s| s.parse::<u64>().ok())
                    .sum();
            }
        }
    }
    0
}

fn free_disk_bytes(path: &std::path::Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        #[repr(C)]
        struct Statvfs {
            f_bsize: u64,
            f_frsize: u64,
            f_blocks: u64,
            f_bfree: u64,
            f_bavail: u64,
            _rest: [u64; 10],
        }
        extern "C" {
            fn statvfs(path: *const i8, buf: *mut Statvfs) -> i32;
        }
        let path_c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stat: Statvfs = unsafe { std::mem::zeroed() };
        if unsafe { statvfs(path_c.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        Some(stat.f_frsize * stat.f_bavail)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

#[derive(Debug, Serialize)]
pub struct PlayerCompatibility {
    pub supports_import_preflight: bool,
    pub supports_upload_mapping: bool,
    pub supports_commit_mapping: bool,
    pub supports_queue_transfer: bool,
    pub queue_restored: bool,
    pub playback_restored: bool,
    pub receiver_e2e_ready: bool,
    pub contract_status: String,
}

impl PlayerCompatibility {
    fn new(
        has_queue: bool,
        playback_restored: bool,
        has_receivers: bool,
        has_imports: bool,
    ) -> Self {
        let status = if has_queue && playback_restored {
            "CONTRACT_OK"
        } else if has_queue {
            "CONTRACT_PARTIAL"
        } else {
            "CONTRACT_OK"
        };
        Self {
            supports_import_preflight: has_imports,
            supports_upload_mapping: has_imports,
            supports_commit_mapping: has_imports,
            supports_queue_transfer: has_queue,
            queue_restored: has_queue,
            playback_restored,
            receiver_e2e_ready: has_receivers,
            contract_status: status.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DiagnosticsReport {
    pub healthy: bool,
    pub db: DbStatus,
    pub library: LibraryStatus,
    pub token_store: TokenStoreStatus,
    pub import_staging: ImportStagingStatus,
    pub playback: PlaybackStatus,
    pub events: EventsStatus,
    pub queues: QueuesStatus,
    pub disk: DiskStatus,
    pub receiver: ReceiverStatus,
    pub player_compatibility: PlayerCompatibility,
    pub config: ConfigStatus,
    pub system: SystemStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub memory_rss_mb: u64,
    pub memory_vm_mb: u64,
    pub binary_size_bytes: u64,
    pub thread_count: u32,
    pub cpu_usage_percent: f64,
    pub uptime_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct DbStatus {
    pub connected: bool,
    pub total_tracks: i64,
    pub total_playlists: i64,
    pub total_devices: i64,
    pub active_import_sessions: i64,
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
    pub restored: bool,
    pub has_queue: bool,
}

#[derive(Debug, Serialize)]
pub struct EventsStatus {
    pub websocket: bool,
    pub auth_enabled: bool,
    pub recommended_polling: bool,
}

#[derive(Debug, Serialize)]
pub struct QueuesStatus {
    pub total_queues: i64,
    pub total_items: i64,
}

#[derive(Debug, Serialize)]
pub struct DiskStatus {
    pub music_path_free_bytes: Option<u64>,
    pub cache_path_free_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ReceiverStatus {
    pub client_available: bool,
    pub registered_receivers: usize,
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

pub async fn diagnostics_handler(State(state): State<AppState>) -> Json<DiagnosticsReport> {
    let mut warnings: Vec<String> = Vec::new();

    let (total_tracks, _total_albums, _total_artists) =
        match michi_db::library_stats(&state.db).await {
            Ok(s) => (s.tracks, s.albums, s.artists),
            Err(e) => {
                warnings.push(format!("library_stats failed: {}", e));
                (0, 0, 0)
            }
        };

    let total_devices = michi_db::list_link_devices(&state.db)
        .await
        .map(|d| d.len() as i64)
        .unwrap_or(0);
    let total_playlists = michi_db::list_playlists(&state.db, None)
        .await
        .map(|p| p.len() as i64)
        .unwrap_or(0);

    let configured_paths: Vec<String> = state
        .config
        .music_paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let paths_exist: Vec<bool> = state
        .config
        .music_paths
        .iter()
        .map(|p| p.exists())
        .collect();

    let staging_path = state
        .config
        .music_paths
        .first()
        .map(|p| p.join(".import"))
        .map(|p| p.to_string_lossy().to_string());
    let staging_exists = staging_path
        .as_ref()
        .map(|p| std::path::Path::new(p).exists())
        .unwrap_or(false);
    let staging_size = if staging_exists {
        dir_size(std::path::Path::new(staging_path.as_ref().unwrap())).unwrap_or(0)
    } else {
        0
    };

    let active_import_sessions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM import_sessions WHERE state NOT IN ('committed', 'rolled_back')",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let registered_receivers = state
        .receiver_manager
        .registry()
        .await
        .read()
        .await
        .list()
        .len();

    let active_token_count = total_devices.max(0) as usize;

    // Compute CPU% with a 200ms sample window
    let cpu_percent = {
        let (before_proc, before_total) =
            tokio::task::spawn_blocking(|| (read_cpu_ticks(), read_total_cpu_ticks()))
                .await
                .unwrap_or((0, 0));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let (after_proc, after_total) =
            tokio::task::spawn_blocking(|| (read_cpu_ticks(), read_total_cpu_ticks()))
                .await
                .unwrap_or((0, 0));
        let proc_delta = after_proc.saturating_sub(before_proc);
        let total_delta = after_total.saturating_sub(before_total);
        let num_cores = std::thread::available_parallelism()
            .map(|n| n.get() as f64)
            .unwrap_or(1.0);
        if total_delta > 0 {
            (proc_delta as f64 / total_delta as f64) * 100.0 * num_cores
        } else {
            0.0
        }
    };

    let playback = state.playback_state.read().await;

    let total_queues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM queues")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
    let total_queue_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM queue_items")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    // Check if playback was restored
    let playback_restored = michi_db::get_latest_playback_session(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.restored)
        .unwrap_or(false);

    // Disk free
    let music_free = state.config.music_paths.first().and_then(|p| {
        if p.exists() {
            free_disk_bytes(p)
        } else {
            None
        }
    });
    let cache_free = if state.config.cache_path.exists() {
        free_disk_bytes(&state.config.cache_path)
    } else {
        None
    };

    // Warnings
    if !state.config.music_paths.iter().any(|p| p.exists()) {
        warnings.push("no music paths exist on disk".into());
    }
    if state.config.auth_enabled && state.config.auth_username.is_none() {
        warnings.push("auth enabled but no username configured".into());
    }
    if staging_size > 1_000_000_000 {
        warnings.push(format!(
            ".import staging is large ({} MB). Commits pending?",
            staging_size / 1_000_000
        ));
    }
    if let Some(free) = music_free {
        if free < 1_000_000_000 {
            warnings.push(format!(
                "low disk space on music path: {} MB free",
                free / 1_000_000
            ));
        }
    }

    let (memory_rss_mb, memory_vm_mb) = read_memory();
    let thread_count = read_thread_count();
    let binary_size_bytes = read_binary_size();
    let uptime_seconds = state.started_at.elapsed().as_secs();

    Json(DiagnosticsReport {
        healthy: warnings.is_empty(),
        db: DbStatus {
            connected: !state.db.is_closed(),
            total_tracks,
            total_playlists,
            total_devices,
            active_import_sessions,
        },
        library: LibraryStatus {
            configured_paths,
            paths_exist,
            total_tracks,
        },
        token_store: TokenStoreStatus {
            active_tokens: active_token_count,
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
            restored: playback_restored,
            has_queue: total_queues > 0,
        },
        events: EventsStatus {
            websocket: true,
            auth_enabled: state.config.auth_enabled,
            recommended_polling: true,
        },
        queues: QueuesStatus {
            total_queues,
            total_items: total_queue_items,
        },
        disk: DiskStatus {
            music_path_free_bytes: music_free,
            cache_path_free_bytes: cache_free,
        },
        receiver: ReceiverStatus {
            client_available: registered_receivers > 0,
            registered_receivers,
        },
        player_compatibility: PlayerCompatibility::new(
            total_queues > 0,
            playback_restored,
            registered_receivers > 0,
            active_import_sessions > 0,
        ),
        system: SystemStatus {
            memory_rss_mb,
            memory_vm_mb,
            binary_size_bytes,
            thread_count,
            cpu_usage_percent: cpu_percent,
            uptime_seconds,
        },
        config: ConfigStatus {
            port: state.config.port(),
            music_paths: state
                .config
                .music_paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            config_path: state.config.config_path.to_string_lossy().to_string(),
            cache_path: state.config.cache_path.to_string_lossy().to_string(),
            database_url: state.config.database_url.clone(),
            auth_enabled: state.config.auth_enabled,
            dev_mode: state.config.dev_mode,
            server_id: state.config.server_id.to_string(),
        },
        warnings,
    })
}

fn dir_size(path: &std::path::Path) -> Option<u64> {
    let mut total: u64 = 0;
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
