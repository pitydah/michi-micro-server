use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    http::HeaderMap,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use michi_config::Config;
use michi_security::SecurityState;
use michi_sync::PlaybackState;
use michi_sync::SyncManager;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use tokio_tungstenite::tungstenite::Message;
use tower_http::cors::CorsLayer;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use utoipa::OpenApi;
use uuid::Uuid;

mod auth;
mod library;
mod openapi;
mod players;
mod pwa;
mod rooms;
mod root;
mod scrobble;
mod static_files;
mod status;
mod stream;
mod sync_api;
mod sync_ws;
mod transcode;
mod ws;

pub mod routes;
pub use routes::v1::audit::record_audit;

use openapi::ApiDoc;

pub use status::StatusResponse;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Config,
    pub db: SqlitePool,
    pub tx: broadcast::Sender<String>,
    pub playback_state: Arc<RwLock<PlaybackState>>,
    pub sync_tx: broadcast::Sender<michi_sync::SyncMessage>,
    pub auth_sessions: auth::AuthState,
    pub auth_enabled: bool,
    pub admin_user_id: Option<Uuid>,
    pub started_at: Instant,
    pub transcode_profiles: Arc<RwLock<Vec<crate::transcode::TranscodeProfile>>>,
    pub token_store: michi_link::TokenStore,
    pub receiver_manager: michi_receivers::ReceiverSessionManager,
    pub sync_manager: SyncManager,
    pub security_state: SecurityState,
    pub disabled_modules: Arc<RwLock<HashSet<String>>>,
    pub shutdown_token: CancellationToken,
    pub scan_cancel: CancellationToken,
    pub sync_cancel: CancellationToken,
    pub playback_cancel: CancellationToken,
    pub backup_cancel: CancellationToken,
    pub webhook_cancel: CancellationToken,
    pub homeassistant_cancel: CancellationToken,
}

impl AppState {
    pub fn new(config: Config, db: SqlitePool, admin_user_id: Option<Uuid>) -> Self {
        let (tx, _) = broadcast::channel(64);
        let (sync_tx, _) = broadcast::channel(64);
        let auth_sessions = auth::AuthState::new();
        let auth_enabled = config.auth_enabled;
        if auth_enabled {
            auth::spawn_session_cleanup(auth_sessions.clone());
        }
        let token_store = michi_link::TokenStore::new();
        let db_for_tokens = db.clone();
        let ts = token_store.clone();
        tokio::spawn(async move {
            match michi_link::load_tokens_from_db(&ts, &db_for_tokens).await {
                Ok(n) => tracing::info!("loaded {} device tokens from DB", n),
                Err(e) => tracing::warn!("failed to load device tokens from DB: {}", e),
            }
        });
        michi_link::spawn_token_cleanup(token_store.clone());
        let playback_state = Arc::new(RwLock::new(PlaybackState::default()));
        let receiver_manager = michi_receivers::ReceiverSessionManager::new();
        let upload_dir = config.cache_path.join("uploads");
        let _ = std::fs::create_dir_all(&upload_dir);
        let sync_manager = michi_sync::SyncManager::new(db.clone(), upload_dir);
        let security_config = michi_security::SecurityConfig::default();
        let security_state = michi_security::SecurityState::new(security_config);

        let disabled_modules: Arc<RwLock<HashSet<String>>> = Arc::new(RwLock::new(HashSet::new()));
        let shutdown_token = CancellationToken::new();

        let scan_cancel = CancellationToken::new();
        let sync_cancel = CancellationToken::new();
        let playback_cancel = CancellationToken::new();
        let backup_cancel = CancellationToken::new();
        let webhook_cancel = CancellationToken::new();
        let homeassistant_cancel = CancellationToken::new();

        let state = Self {
            config,
            db,
            tx,
            playback_state,
            sync_tx,
            auth_sessions,
            auth_enabled,
            admin_user_id,
            started_at: Instant::now(),
            transcode_profiles: Arc::new(RwLock::new(crate::transcode::default_profiles())),
            token_store,
            receiver_manager,
            sync_manager,
            security_state,
            disabled_modules,
            shutdown_token,
            scan_cancel,
            sync_cancel,
            playback_cancel,
            backup_cancel,
            webhook_cancel,
            homeassistant_cancel,
        };

        state.spawn_background_tasks();
        state
    }

    fn spawn_background_tasks(&self) {
        let db = self.db.clone();
        let dm = self.disabled_modules.clone();
        let shutdown = self.shutdown_token.clone();

        // Import cleanup (siempre corre)
        routes::v1::import::spawn_import_cleanup(&self.config, db.clone());
        // Restore playback state (siempre corre)
        routes::v1::playback::auto_restore_playback_state(db.clone(), self.playback_state.clone());

        // DB maintenance scheduler (siempre corre)
        let maintenance_db = db.clone();
        let maint_shutdown = shutdown.clone();
        tokio::spawn(async move {
            let mut hourly = tokio::time::interval(Duration::from_secs(3600));
            let mut daily = tokio::time::interval(Duration::from_secs(86400));
            let mut weekly = tokio::time::interval(Duration::from_secs(604800));
            hourly.tick().await;
            daily.tick().await;
            weekly.tick().await;
            loop {
                tokio::select! {
                    _ = maint_shutdown.cancelled() => break,
                    _ = hourly.tick() => {
                        let _ = michi_db::run_hourly_maintenance(&maintenance_db).await;
                    }
                    _ = daily.tick() => {
                        let _ = michi_db::run_daily_maintenance(&maintenance_db).await;
                    }
                    _ = weekly.tick() => {
                        let _ = michi_db::run_weekly_maintenance(&maintenance_db).await;
                    }
                }
            }
            info!("DB maintenance scheduler stopped");
        });

        // Integrity cron (solo si scan habilitado)
        let integrity_db = db.clone();
        let integrity_shutdown = shutdown.clone();
        let integrity_dm = dm.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(86400));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = integrity_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        if integrity_dm.read().await.contains("scan") {
                            continue;
                        }
                        tracing::info!("integrity check: starting daily scan");
                        let tracks = match michi_db::list_tracks(&integrity_db).await {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::warn!("integrity check: db error: {}", e);
                                continue;
                            }
                        };
                        let mut missing = 0u64;
                        for track in &tracks {
                            if !std::path::Path::new(&track.file_path).exists() {
                                missing += 1;
                                tracing::warn!("integrity: missing file: {}", track.file_path);
                            }
                        }
                        tracing::info!(
                            "integrity check: {}/{} files ok, {} missing",
                            tracks.len() - missing as usize,
                            tracks.len(),
                            missing
                        );
                    }
                }
            }
            info!("integrity cron stopped");
        });

        // Library watcher (solo si scan habilitado)
        let watch_paths = self.config.music_paths.clone();
        let watch_db = db.clone();
        let watch_shutdown = shutdown.clone();
        let watch_dm = dm.clone();
        let watch_cancel = self.scan_cancel.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = watch_cancel.cancelled() => {
                    info!("scan module cancelled at startup, watcher not started");
                }
                _ = async {
                    if watch_dm.read().await.contains("scan") {
                        info!("scan module disabled, watcher not started");
                        // Keep alive so we can react to re-enable
                        futures_util::future::pending::<()>().await;
                    }
                    let watcher = michi_scanner::watcher::LibraryWatcher::new(watch_paths, watch_db);
                    watcher.start().await;
                } => {}
            }
            info!("watcher stopped");
        });

        // Receiver heartbeat monitor (siempre corre)
        let rm = self.receiver_manager.clone();
        let hb_db = db.clone();
        let hb_shutdown = shutdown.clone();
        let hb_dm = dm.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = hb_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        if hb_dm.read().await.contains("playback") {
                            continue;
                        }
                        let reg = rm.registry().await;
                        let reg_read = reg.read().await;
                        let now = Utc::now();
                        let mut candidates: Vec<(String, chrono::DateTime<Utc>, String)> = Vec::new();
                        for e in reg_read.list() {
                            if let Some(last) = e.last_seen {
                                if (now - last).num_seconds() > 120 {
                                    candidates.push((
                                        e.receiver_id.clone(),
                                        last,
                                        e.base_url.clone(),
                                    ));
                                }
                            }
                        }
                        drop(reg_read);

                        for (recv_id, _last_seen, base_url) in candidates {
                            let mut reg_write = reg.write().await;
                            let should_ping = reg_write
                                .get(&recv_id)
                                .and_then(|e| e.active_session_id.as_ref())
                                .is_some();
                            drop(reg_write);

                            if should_ping {
                                let url = format!("{}/api/v1/receivers/{}/heartbeat", base_url, &recv_id);
                                match reqwest::Client::new()
                                    .post(&url)
                                    .timeout(Duration::from_secs(5))
                                    .send()
                                    .await
                                {
                                    Ok(_) => {
                                        let mut reg_w2 = reg.write().await;
                                        if let Some(e) = reg_w2.get_mut(&recv_id) {
                                            e.last_seen = Some(Utc::now());
                                        }
                                    }
                                    Err(_) => {
                                        let mut reg_w2 = reg.write().await;
                                        if let Some(e) = reg_w2.get_mut(&recv_id) {
                                            e.active_session_id = None;
                                        }
                                        record_audit(
                                            &hb_db,
                                            "receiver_offline",
                                            Some("receiver"),
                                            Some(&recv_id),
                                            None,
                                        )
                                        .await;
                                    }
                                }
                            } else {
                                let mut reg_w2 = reg.write().await;
                                if let Some(e) = reg_w2.get_mut(&recv_id) {
                                    e.active_session_id = None;
                                }
                            }
                        }
                    }
                }
            }
            info!("heartbeat monitor stopped");
        });

        // Job Queue supervisor
        let supervisor_db = db.clone();
        let supervisor_state = self.clone();
        let supervisor_shutdown = shutdown.clone();
        let supervisor_dm = dm.clone();
        tokio::spawn(async move {
            let max_jobs: usize = std::env::var("MICHI_MAX_JOBS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3);
            let semaphore = Arc::new(tokio::sync::Semaphore::new(max_jobs));
            let mut interval = tokio::time::interval(Duration::from_secs(2));

            loop {
                tokio::select! {
                    _ = supervisor_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        if supervisor_dm.read().await.contains("scan") && supervisor_dm.read().await.contains("sync") {
                            continue;
                        }
                        let pending = match michi_db::get_pending_jobs(&supervisor_db, 5).await {
                            Ok(jobs) => jobs,
                            Err(e) => {
                                tracing::warn!("job supervisor: failed to query pending jobs: {}", e);
                                continue;
                            }
                        };
                        for job in &pending {
                            let permit = match semaphore.clone().acquire_owned().await {
                                Ok(p) => p,
                                Err(_) => break,
                            };
                            let claimed = match michi_db::claim_job(&supervisor_db, &job.id).await {
                                Ok(true) => true,
                                Ok(false) => continue,
                                Err(e) => {
                                    tracing::warn!("job supervisor: failed to claim job {}: {}", job.id, e);
                                    continue;
                                }
                            };
                            if !claimed {
                                continue;
                            }

                            let worker_db = supervisor_db.clone();
                            let worker_state = supervisor_state.clone();
                            let job_id = job.id.clone();
                            let job_kind = job.kind.clone();
                            let job_payload = job.payload.clone();
                            let worker_dm = supervisor_dm.clone();

                            tokio::spawn(async move {
                                let _permit = permit;
                                let result = run_job_worker(
                                    &worker_db,
                                    &worker_state,
                                    &job_id,
                                    &job_kind,
                                    job_payload.as_ref(),
                                    &worker_dm,
                                )
                                .await;
                                match result {
                                    Ok(msg) => {
                                        tracing::info!("job {} completed: {}", job_id, msg);
                                        let _ = michi_db::complete_job(&worker_db, &job_id).await;
                                    }
                                    Err(e) => {
                                        tracing::error!("job {} failed: {}", job_id, e);
                                        let _ = michi_db::fail_job(&worker_db, &job_id, &e).await;
                                    }
                                }
                            });
                        }
                    }
                }
            }
            info!("job supervisor stopped");
        });

        // Start sync peers (solo si sync habilitado)
        // Se hace desde main.rs después de AppState::new()
    }

    pub fn server_id(&self) -> Uuid {
        self.config.server_id
    }

    pub async fn get_user_id(&self, headers: &HeaderMap) -> Option<Uuid> {
        if !self.auth_enabled {
            return None;
        }
        let auth_header = headers.get("Authorization")?.to_str().ok()?;
        let token = auth_header.strip_prefix("Bearer ")?;
        self.auth_sessions.extract_user_id(token).await
    }
}

async fn run_job_worker(
    db: &SqlitePool,
    state: &AppState,
    job_id: &str,
    kind: &str,
    payload: Option<&serde_json::Value>,
    _dm: &Arc<RwLock<HashSet<String>>>,
) -> Result<String, String> {
    match kind {
        "scan" => {
            tracing::info!("job {}: starting library scan", job_id);
            let paths = &state.config.music_paths;
            let tracks = michi_scanner::scan_directories(paths).await;
            let total = tracks.len();
            for (i, track) in tracks.iter().enumerate() {
                michi_db::upsert_track(db, track)
                    .await
                    .map_err(|e| format!("upsert error: {}", e))?;
                if i % 50 == 0 {
                    let progress = (i as f64) / (total as f64).max(1.0);
                    let _ = michi_db::update_job_progress(db, job_id, progress).await;
                }
            }
            // Detect deletions
            if let Ok(db_tracks) = michi_db::list_tracks(db).await {
                let scanned_ids: HashSet<_> = tracks.iter().map(|t| t.id).collect();
                for old in &db_tracks {
                    if !scanned_ids.contains(&old.id) {
                        let _ = michi_db::delete_track(db, &old.id).await;
                    }
                }
            }
            record_audit(db, "scan_completed", Some("library"), None, None).await;
            Ok(format!("scanned {} tracks", total))
        }
        "sync" => {
            tracing::info!("job {}: triggering sync", job_id);
            // Sync is triggered via WebSocket; the broadcast sends a Ping
            // which peers interpret as a sync heartbeat
            let _ = state.sync_tx.send(michi_sync::SyncMessage::Ping);
            record_audit(db, "sync_triggered", Some("sync"), None, None).await;
            Ok("sync triggered".to_string())
        }
        "backup" => {
            tracing::info!("job {}: running backup", job_id);
            let tracks = michi_db::list_tracks(db)
                .await
                .map_err(|e| format!("list tracks error: {}", e))?;
            let playlists = michi_db::list_playlists(db, None)
                .await
                .map_err(|e| format!("list playlists error: {}", e))?;
            let output = serde_json::json!({
                "exported_at": Utc::now().to_rfc3339(),
                "tracks_count": tracks.len(),
                "playlists_count": playlists.len(),
            });
            record_audit(
                db,
                "backup_completed",
                Some("backup"),
                None,
                Some(output),
            )
            .await;
            let _ = michi_db::update_job_progress(db, job_id, 1.0).await;
            Ok(format!("backup complete: {} tracks, {} playlists", tracks.len(), playlists.len()))
        }
        "cleanup" => {
            tracing::info!("job {}: running cleanup", job_id);
            michi_db::run_hourly_maintenance(db)
                .await
                .map_err(|e| format!("cleanup error: {}", e))?;
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(db)
                .await
                .map_err(|e| format!("checkpoint error: {}", e))?;
            let _ = michi_db::update_job_progress(db, job_id, 0.5).await;
            // Clean stale jobs older than 7 days
            sqlx::query(
                "DELETE FROM job_queue WHERE created_at < datetime('now', '-7 days') AND state IN ('completed', 'failed', 'cancelled')"
            )
                .execute(db)
                .await
                .ok();
            let _ = michi_db::update_job_progress(db, job_id, 1.0).await;
            record_audit(db, "cleanup_completed", Some("system"), None, None).await;
            Ok("cleanup complete".to_string())
        }
        "custom" => {
            tracing::info!("job {}: executing custom SQL", job_id);
            let sql = payload
                .and_then(|p| p.get("sql"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "custom job requires payload.sql field".to_string())?;
            sqlx::query(sql)
                .execute(db)
                .await
                .map_err(|e| format!("custom SQL error: {}", e))?;
            let _ = michi_db::update_job_progress(db, job_id, 1.0).await;
            record_audit(db, "custom_job_executed", Some("system"), None, payload.cloned()).await;
            Ok("custom SQL executed".to_string())
        }
        _ => Err(format!("unknown job kind: {}", kind)),
    }
}

pub async fn init_admin_user(config: &Config, db: &SqlitePool) -> Option<Uuid> {
    if !config.auth_enabled {
        return None;
    }
    let username = config.auth_username.as_deref()?;
    let password = config.auth_password.as_deref()?;

    match michi_db::get_user_by_username(db, username)
        .await
        .ok()
        .flatten()
    {
        Some((id, _, _, _)) => Some(id),
        None => {
            let id = Uuid::new_v4();
            match auth::hash_password(password) {
                Ok(hash) => {
                    if michi_db::create_user(db, &id, username, &hash, true)
                        .await
                        .is_ok()
                    {
                        info!("created admin user: {}", username);
                        Some(id)
                    } else {
                        warn!("failed to create admin user");
                        None
                    }
                }
                Err(e) => {
                    warn!("failed to hash admin password: {}", e);
                    None
                }
            }
        }
    }
}

pub fn start_sync_peers(state: &AppState) {
    let peers = state.config.sync_peers.clone();
    let sync_name = state.config.sync_name.clone();
    let sync_tx = state.sync_tx.clone();
    let tx = state.tx.clone();
    let playback_state = state.playback_state.clone();
    let shutdown = state.shutdown_token.clone();
    let dm = state.disabled_modules.clone();
    let sync_cancel = state.sync_cancel.clone();

    tokio::spawn(async move {
        tokio::select! {
            _ = sync_cancel.cancelled() => {
                info!("sync module cancelled at startup, sync peers not started");
            }
            _ = async {
                if dm.read().await.contains("sync") {
                    info!("sync module disabled, sync peers not started");
                    futures_util::future::pending::<()>().await;
                }
                for peer in &peers {
                    let peer = peer.clone();
                    let sync_name = sync_name.clone();
                    let sync_tx = sync_tx.clone();
                    let tx = tx.clone();
                    let playback_state = playback_state.clone();
                    let peer_shutdown = shutdown.clone();
                    let peer_dm = dm.clone();

                    tokio::spawn(async move {
                        let url = format!("ws://{}/api/sync", peer);
                        let mut attempt = 0u64;

                        loop {
                            tokio::select! {
                                _ = peer_shutdown.cancelled() => break,
                                _ = async {
                                    if peer_dm.read().await.contains("sync") {
                                        tokio::time::sleep(Duration::from_secs(5)).await;
                                        return;
                                    }
                                    info!("connecting to sync peer: {} (attempt {})", url, attempt + 1);
                                    match tokio_tungstenite::connect_async(&url).await {
                                        Ok((ws_stream, _)) => {
                                            info!("connected to sync peer: {}", peer);
                                            attempt = 0;
                                            let (mut sender, mut receiver) = ws_stream.split();
                                            let mut local_sync_rx = sync_tx.subscribe();

                                            let identify = michi_sync::SyncMessage::Identify {
                                                name: sync_name.clone(),
                                                version: env!("CARGO_PKG_VERSION").into(),
                                                device_type: michi_sync::DeviceType::Server,
                                            };
                                            if let Ok(json) = identify.serialize() {
                                                let _ = sender.send(Message::Text(json)).await;
                                            }

                                            let send_task = tokio::spawn(async move {
                                                while let Ok(msg) = local_sync_rx.recv().await {
                                                    if let Ok(json) = msg.serialize() {
                                                        if sender.send(Message::Text(json)).await.is_err() {
                                                            break;
                                                        }
                                                    }
                                                }
                                            });

                                            let recv_tx = tx.clone();
                                            let recv_playback = playback_state.clone();
                                            let recv_task = tokio::spawn(async move {
                                                while let Some(Ok(msg)) = receiver.next().await {
                                                    match msg {
                                                        Message::Text(text) => {
                                                            if let Ok(michi_sync::SyncMessage::State {
                                                                track_id,
                                                                position_ms,
                                                                playing,
                                                                volume,
                                                                ..
                                                            }) = michi_sync::SyncMessage::deserialize(&text)
                                                            {
                                                                let new_state = michi_sync::PlaybackState {
                                                                    track_id,
                                                                    position_ms,
                                                                    playing,
                                                                    volume,
                                                                    updated_at: chrono::Utc::now(),
                                                                    playlist_id: None,
                                                                    queue_position: None,
                                                                    device_id: None,
                                                                };
                                                                {
                                                                    let mut current = recv_playback.write().await;
                                                                    *current = new_state;
                                                                }
                                                                let tid = track_id
                                                                    .map(|id| format!("\"{}\"", id))
                                                                    .unwrap_or_else(|| "null".into());
                                                                let msg = format!(
                                                                    "{{\"type\":\"sync_state\",\
                                                                     \"track_id\":{tid},\
                                                                     \"position_ms\":{pos},\
                                                                     \"playing\":{play},\
                                                                     \"volume\":{vol}}}",
                                                                    pos = position_ms,
                                                                    play = playing,
                                                                    vol = volume,
                                                                );
                                                                let _ = recv_tx.send(msg);
                                                            }
                                                        }
                                                        Message::Close(_) => break,
                                                        _ => {}
                                                    }
                                                }
                                            });

                                            tokio::select! {
                                                _ = send_task => {},
                                                _ = recv_task => {},
                                            }
                                        }
                                        Err(e) => {
                                            warn!(
                                                "failed to connect to sync peer {} (attempt {}): {}",
                                                peer,
                                                attempt + 1,
                                                e
                                            );
                                        }
                                    }

                                    attempt += 1;
                                    let delay = Duration::from_secs(
                                        std::cmp::min(attempt * 5, 300) + rand::random::<u64>() % 10,
                                    );
                                    info!("sync peer {}: reconnecting in {}s", peer, delay.as_secs());
                                    tokio::time::sleep(delay).await;
                                } => {}
                            }
                        }
                        info!("sync peer {} stopped", peer);
                    });
                }
                futures_util::future::pending::<()>().await;
            } => {}
        }
    });
}

fn v1_link_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/server/info",
            get(routes::v1::server::server_info_handler),
        )
        .route("/api/v1/status", get(routes::v1::server::status_handler))
        .route(
            "/api/v1/pair/start",
            post(routes::v1::pair::link_pair_start),
        )
        .route(
            "/api/v1/pair/confirm",
            post(routes::v1::pair::link_pair_confirm),
        )
        .route(
            "/api/v1/token/refresh",
            post(routes::v1::pair::link_token_refresh),
        )
        .route(
            "/api/v1/pair/qr",
            post(routes::v1::pair::qr_generate_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/svg",
            get(routes::v1::pair::qr_svg_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/claim",
            post(routes::v1::pair::qr_claim_handler),
        )
        .route(
            "/api/v1/devices/revoke",
            post(routes::v1::pair::link_devices_revoke),
        )
        .route(
            "/api/v1/link/devices",
            get(routes::v1::pair::list_devices_handler),
        )
        .route(
            "/api/v1/library/stats",
            get(routes::v1::library::library_stats_handler),
        )
        .route(
            "/api/v1/library/health",
            get(routes::v1::library::library_health_handler),
        )
        .route(
            "/api/v1/library/scan",
            post(routes::v1::library::library_scan_handler),
        )
        .route("/api/v1/tracks", get(routes::v1::tracks::tracks_handler))
        .route("/api/v1/tracks/:id", get(routes::v1::tracks::track_handler))
        .route("/api/v1/search", get(routes::v1::tracks::search_handler))
        .route(
            "/api/v1/search/advanced",
            get(routes::v1::search::search_advanced_handler),
        )
        .route(
            "/api/v1/stream/:id",
            get(routes::v1::stream::stream_handler),
        )
        .route(
            "/api/v1/download/:id",
            get(routes::v1::stream::download_handler),
        )
        .route(
            "/api/v1/artwork/:id",
            get(routes::v1::artwork::artwork_handler),
        )
        .route(
            "/api/v1/playlists",
            get(routes::v1::playlists::playlists_handler)
                .post(routes::v1::playlists::create_playlist_handler),
        )
        .route(
            "/api/v1/playlists/:id",
            get(routes::v1::playlists::get_playlist_handler)
                .put(routes::v1::playlists::update_playlist_handler)
                .delete(routes::v1::playlists::delete_playlist_handler),
        )
        .route(
            "/api/v1/playlists/:id/export/m3u",
            get(routes::v1::playlists::export_playlist_m3u_handler),
        )
        .route(
            "/api/v1/playlists/smart",
            post(routes::v1::playlists::smart_playlist_handler),
        )
        .route(
            "/api/v1/chains",
            get(routes::v1::chains::list_chains_handler)
                .post(routes::v1::chains::create_chain_handler),
        )
        .route(
            "/api/v1/chains/:id",
            get(routes::v1::chains::get_chain_handler)
                .put(routes::v1::chains::update_chain_handler)
                .delete(routes::v1::chains::delete_chain_handler),
        )
        .route(
            "/api/v1/chains/:id/links",
            post(routes::v1::chains::add_link_handler),
        )
        .route(
            "/api/v1/chains/:chain_id/links/:link_id",
            put(routes::v1::chains::update_link_handler)
                .delete(routes::v1::chains::delete_link_handler),
        )
        .route(
            "/api/v1/chains/:id/links/reorder",
            post(routes::v1::chains::reorder_links_handler),
        )
        .route(
            "/api/v1/chains/:id/play",
            post(routes::v1::chains::play_chain_handler),
        )
        .route(
            "/api/v1/chains/:id/stop",
            post(routes::v1::chains::stop_chain_handler),
        )
        .route(
            "/api/v1/chains/:id/volume",
            post(routes::v1::chains::chain_volume_handler),
        )
        .route(
            "/api/v1/starred",
            get(routes::v1::favorites::starred_tracks_handler),
        )
        .route(
            "/api/v1/star/:id",
            post(routes::v1::favorites::star_track_handler),
        )
        .route(
            "/api/v1/rate/:id",
            post(routes::v1::favorites::rate_track_handler),
        )
        .route(
            "/api/v1/sync/manifest",
            get(routes::v1::sync::sync_manifest_handler),
        )
        .route(
            "/api/v1/sync/manifest/delta",
            get(routes::v1::sync::sync_manifest_delta_handler),
        )
        .route(
            "/api/v1/sync/state",
            post(routes::v1::sync::sync_state_handler),
        )
        .route(
            "/api/v1/sync/upload/init",
            post(routes::v1::sync::sync_upload_init_handler),
        )
        .route(
            "/api/v1/sync/upload/:file_id/chunk",
            post(routes::v1::sync::sync_upload_chunk_handler),
        )
        .route(
            "/api/v1/sync/upload/:file_id/status",
            get(routes::v1::sync::sync_upload_status_handler),
        )
        .route(
            "/api/v1/sync/upload/file",
            post(routes::v1::sync::sync_upload_file_handler),
        )
        .route(
            "/api/v1/sync/playlist",
            post(routes::v1::sync::sync_playlist_handler),
        )
        .route(
            "/api/v1/artists/:name/insights",
            get(routes::v1::insights::artist_insights_handler),
        )
        .route(
            "/api/v1/albums/:key/health",
            get(routes::v1::insights::album_health_handler),
        )
        .route(
            "/api/v1/import/session",
            post(routes::v1::import::import_session_handler),
        )
        .route(
            "/api/v1/import/session/create",
            post(routes::v1::import::import_session_handler),
        )
        .route(
            "/api/v1/import/preflight",
            post(routes::v1::import::import_preflight_handler),
        )
        .route(
            "/api/v1/import/upload/:session_id",
            post(routes::v1::import::import_upload_handler),
        )
        .route(
            "/api/v1/import/commit/:session_id",
            post(routes::v1::import::import_commit_handler),
        )
        .route(
            "/api/v1/import/session/commit/:session_id",
            post(routes::v1::import::import_commit_handler),
        )
        .route(
            "/api/v1/import/rollback/:session_id",
            post(routes::v1::import::import_rollback_handler),
        )
        .route(
            "/api/v1/import/session/:session_id/status",
            get(routes::v1::import::import_session_status_handler),
        )
        .route(
            "/api/v1/diagnostics",
            get(routes::v1::diagnostics::diagnostics_handler),
        )
        .route("/api/v1/backup", get(routes::v1::backup::backup_handler))
        .route(
            "/api/v1/home/dashboard",
            get(routes::v1::dashboard::dashboard_handler),
        )
        .route(
            "/api/v1/history",
            get(routes::v1::history::history_handler)
                .delete(routes::v1::history::clear_history_handler),
        )
        .route(
            "/api/v1/history/stats",
            get(routes::v1::history::history_stats_handler),
        )
        .route(
            "/api/v1/history/export",
            get(routes::v1::history::history_export_handler),
        )
        .route(
            "/api/v1/bookmarks",
            get(routes::v1::bookmarks::list_bookmarks_handler)
                .post(routes::v1::bookmarks::upsert_bookmark_handler),
        )
        .route(
            "/api/v1/bookmarks/:track_id",
            get(routes::v1::bookmarks::get_bookmark_handler)
                .delete(routes::v1::bookmarks::delete_bookmark_handler),
        )
        .route(
            "/api/v1/backup/snapshot",
            post(routes::v1::backup::snapshot_handler),
        )
        .route(
            "/api/v1/radio/stations",
            get(routes::v1::radio::list_radio_stations_handler)
                .post(routes::v1::radio::create_radio_station_handler),
        )
        .route(
            "/api/v1/radio/stations/:id",
            put(routes::v1::radio::update_radio_station_handler)
                .delete(routes::v1::radio::delete_radio_station_handler),
        )
        .route(
            "/api/v1/radio/stations/:id/test",
            post(routes::v1::radio::test_radio_station_handler),
        )
        .route(
            "/api/v1/radio/stations/:id/favorite",
            post(routes::v1::radio::toggle_favorite_handler),
        )
        .route(
            "/api/v1/backup/snapshot/last",
            get(routes::v1::backup::last_snapshot_handler),
        )
        .route(
            "/api/v1/backup/bundle",
            get(routes::v1::backup::backup_bundle_handler),
        )
        .route(
            "/api/v1/webhook",
            get(routes::v1::backup::get_webhook_handler)
                .post(routes::v1::backup::set_webhook_handler)
                .delete(routes::v1::backup::delete_webhook_handler),
        )
        .route(
            "/api/v1/webhook/test",
            post(routes::v1::backup::test_webhook_handler),
        )
        .route(
            "/api/v1/health/verify",
            get(routes::v1::backup::verify_integrity_handler),
        )
        .route(
            "/api/v1/health/mounts",
            get(routes::v1::backup::mount_health_handler),
        )
        .route(
            "/api/v1/health/storage",
            get(routes::v1::storage::storage_health_handler),
        )
        .route(
            "/api/v1/config/validate",
            get(routes::v1::validate::config_validate_handler),
        )
        .route(
            "/api/v1/audit/log",
            get(routes::v1::audit::audit_log_handler),
        )
        .route(
            "/api/v1/modules",
            get(routes::v1::modules::modules_handler),
        )
        .route(
            "/api/v1/modules/:name",
            post(routes::v1::modules::toggle_module_handler),
        )
        .route(
            "/api/v1/health/self-test",
            get(routes::v1::modules::self_test_handler),
        )
        .route(
            "/api/v1/capabilities",
            get(routes::v1::modules::capabilities_handler),
        )
        .route(
            "/api/v1/changes",
            get(routes::v1::modules::change_journal_handler),
        )
        .route(
            "/api/v1/policy",
            get(routes::v1::modules::policy_handler),
        )
        .route(
            "/api/v1/policy/lan",
            post(routes::v1::modules::lan_policy_handler),
        )
        .route(
            "/api/v1/stream/handoff/offer",
            post(routes::v1::modules::handoff_handler),
        )
        .route(
            "/api/v1/jobs",
            get(routes::v1::jobs::list_jobs_handler)
                .post(routes::v1::jobs::create_job_handler),
        )
        .route(
            "/api/v1/jobs/:id",
            get(routes::v1::jobs::get_job_handler),
        )
        .route(
            "/api/v1/jobs/:id/cancel",
            post(routes::v1::jobs::cancel_job_handler),
        )
        .route("/health/live", get(routes::v1::server::health_live_handler))
        .route(
            "/health/ready",
            get(routes::v1::server::health_ready_handler),
        )
        .route(
            "/api/v1/playback/state",
            get(routes::v1::playback::playback_state_handler),
        )
        .route(
            "/api/v1/playback/control",
            post(routes::v1::playback::playback_control_handler),
        )
        .route(
            "/api/v1/playback/session",
            post(routes::v1::playback::playback_session_handler),
        )
        .route(
            "/api/v1/playback/session/:session_id",
            get(routes::v1::playback::playback_session_get_handler),
        )
        .route(
            "/api/v1/playback/session/restore",
            post(routes::v1::playback::playback_session_restore_handler),
        )
        .route(
            "/api/v1/player/handoff",
            post(routes::v1::playback::handoff_handler),
        )
        .route(
            "/api/v1/sessions/active",
            get(routes::v1::sessions::active_streams_handler),
        )
        .route(
            "/api/v1/library/duplicates",
            get(routes::v1::duplicates::duplicates_handler),
        )
        .route(
            "/api/v1/player/announce",
            post(routes::v1::announce::announce_handler),
        )
        .route(
            "/api/v1/settings",
            get(routes::v1::settings::get_settings_handler)
                .put(routes::v1::settings::update_settings_handler),
        )
        .route(
            "/api/v1/setup/status",
            get(routes::v1::setup::setup_status_handler),
        )
        .route(
            "/api/v1/setup/scan",
            post(routes::v1::setup::setup_scan_handler),
        )
        .route(
            "/api/v1/setup/fix-perms",
            post(routes::v1::setup::setup_fix_perms_handler),
        )
        .route(
            "/api/v1/sources",
            get(routes::v1::sources::list_sources_handler)
                .post(routes::v1::sources::add_source_handler),
        )
        .route(
            "/api/v1/sources/:id",
            delete(routes::v1::sources::delete_source_handler),
        )
        .route(
            "/api/v1/sources/:source_id/episodes",
            get(routes::v1::sources::get_episodes_handler),
        )
        .route(
            "/api/v1/sources/episodes/:id",
            put(routes::v1::sources::update_episode_handler),
        )
        .route(
            "/api/v1/stream/proxy/:source_id",
            get(routes::v1::sources::proxy_stream_handler),
        )
        .route(
            "/api/v1/stream/proxy/episode/:episode_id",
            get(routes::v1::sources::proxy_episode_handler),
        )
        .route(
            "/api/v1/shares",
            get(routes::v1::shares::list_shares_handler)
                .post(routes::v1::shares::create_share_handler),
        )
        .route(
            "/api/v1/shares/:id",
            delete(routes::v1::shares::delete_share_handler),
        )
        .route("/api/v1/queue", get(routes::v1::queue::queue_handler))
        .route(
            "/api/v1/queue/items",
            post(routes::v1::queue::queue_items_handler),
        )
        .route(
            "/api/v1/queue/jump",
            post(routes::v1::queue::queue_jump_handler),
        )
        .route(
            "/api/v1/queue/transfer",
            post(routes::v1::queue::queue_transfer_handler),
        )
        .route(
            "/api/v1/queue/reorder",
            put(routes::v1::queue::queue_reorder_handler),
        )
        .route(
            "/api/v1/queue/:queue_id",
            delete(routes::v1::queue::queue_delete_handler),
        )
        .route(
            "/api/v1/queue/save",
            post(routes::v1::queue::queue_save_handler),
        )
        .route(
            "/api/v1/queue/saved",
            get(routes::v1::queue::queue_saved_handler),
        )
        .route(
            "/api/v1/receivers",
            get(routes::v1::receivers::receivers_handler),
        )
        .route(
            "/api/v1/receivers/discover",
            post(routes::v1::receivers::discover_receiver_handler),
        )
        .route(
            "/api/v1/receivers/:id",
            get(routes::v1::receivers::get_receiver_handler),
        )
        .route(
            "/api/v1/receivers/:id/session/start",
            post(routes::v1::receivers::receiver_session_start_handler),
        )
        .route(
            "/api/v1/receivers/:id/session/stop",
            post(routes::v1::receivers::receiver_session_stop_handler),
        )
        .route(
            "/api/v1/receivers/:id/volume",
            post(routes::v1::receivers::receiver_volume_handler),
        )
        .route(
            "/api/v1/receivers/:id/heartbeat",
            post(routes::v1::receivers::receiver_heartbeat_handler),
        )
        .route(
            "/api/v1/devices/discover",
            post(routes::v1::receivers::discover_mdns_handler),
        )
        .route(
            "/api/v1/receivers/groups",
            get(routes::v1::receivers::list_groups_handler)
                .post(routes::v1::receivers::create_group_handler),
        )
        .route(
            "/api/v1/receivers/groups/:group_id/sync",
            post(routes::v1::receivers::sync_group_handler),
        )
        .route(
            "/api/v1/rooms",
            get(routes::v1::rooms::rooms_handler).post(routes::v1::rooms::create_room_handler),
        )
        .route(
            "/api/v1/rooms/:id/play",
            post(routes::v1::rooms::room_play_handler),
        )
        .route(
            "/api/v1/rooms/groups",
            get(routes::v1::receivers::list_room_groups_handler)
                .post(routes::v1::receivers::create_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id",
            get(routes::v1::receivers::get_room_group_handler)
                .put(routes::v1::receivers::update_room_group_handler)
                .delete(routes::v1::receivers::delete_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id/activate",
            post(routes::v1::receivers::activate_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id/deactivate",
            post(routes::v1::receivers::deactivate_room_group_handler),
        )
        .route(
            "/api/v1/rooms/groups/:id/mode",
            post(routes::v1::receivers::set_room_mode_handler),
        )
        .route("/api/v1/events", get(routes::v1::events::events_handler))
        .route(
            "/api/v1/events/sse",
            get(routes::v1::events::events_sse_handler),
        )
}

pub fn create_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/status", get(status::status_handler))
        .route("/api/library/scan", post(library::scan_handler))
        .route("/api/library/stats", get(library::stats_handler))
        .route(
            "/api/library/tracks",
            delete(library::delete_all_tracks_handler),
        )
        .route("/api/tracks", get(library::tracks_handler))
        .route("/api/search", get(library::search_handler))
        .route(
            "/api/tracks/:id",
            get(library::track_handler)
                .delete(library::delete_track_handler)
                .put(library::update_track_handler),
        )
        .route("/api/albums", get(library::albums_handler))
        .route("/api/artists", get(library::artists_handler))
        .route("/api/albums/:album", get(library::album_tracks_handler))
        .route("/api/artists/:artist", get(library::artist_tracks_handler))
        .route("/api/artwork/:id", get(library::artwork_handler))
        .route(
            "/api/playlists",
            get(library::playlists_handler).post(library::create_playlist_handler),
        )
        .route(
            "/api/playlists/:id",
            get(library::get_playlist_handler).delete(library::delete_playlist_handler),
        )
        .route(
            "/api/playlists/:playlist_id/tracks/:track_id",
            post(library::add_playlist_track_handler)
                .delete(library::remove_playlist_track_handler),
        )
        .route(
            "/api/playlists/:id/tracks",
            get(library::get_playlist_tracks_handler),
        )
        .route(
            "/api/playlists/:id/reorder",
            put(library::reorder_playlist_handler),
        )
        .route(
            "/api/playlists/:id/export",
            get(library::export_playlist_handler),
        )
        .route(
            "/api/playlists/import",
            post(library::import_playlist_handler),
        )
        .route(
            "/api/playlists/:id/share",
            get(library::get_share_handler)
                .post(library::enable_share_handler)
                .delete(library::disable_share_handler),
        )
        .route("/api/ws", get(ws::ws_handler))
        .route("/api/sync", get(sync_ws::sync_handler))
        .route(
            "/api/playback/state",
            get(library::get_playback_state_handler).post(library::set_playback_state_handler),
        )
        .route("/api/stream/:id", get(stream::stream_handler))
        .route("/api/playback/record", post(scrobble::record_play_handler))
        .route("/api/history", get(scrobble::history_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.security_state.clone(),
            michi_security::rate_limit_middleware,
        ))
        .layer(middleware::from_fn(
            michi_security::security_headers_middleware,
        ))
        .with_state(state.clone());

    Router::new()
        .route("/", get(root::root_handler))
        .route("/static/styles.css", get(static_files::styles_css))
        .route("/static/app.js", get(static_files::app_js))
        .route("/static/assets/michi-logo.svg", get(static_files::logo_svg))
        .route(
            "/static/assets/michi-micro-server.svg",
            get(static_files::favicon_svg),
        )
        .route(
            "/static/assets/michi-micro-server.png",
            get(static_files::favicon_png),
        )
        .route("/static/i18n/:lang", get(static_files::i18n_handler))
        .route("/manifest.json", get(pwa::manifest_json))
        .route("/sw.js", get(pwa::sw_js))
        .route("/api/shared/:code", get(library::shared_playlist_handler))
        .merge(auth::auth_router())
        .merge(
            utoipa_swagger_ui::SwaggerUi::new("/api/docs")
                .url("/api-docs/openapi.json", ApiDoc::openapi()),
        )
        .merge(protected)
        .merge(sync_api::sync_router())
        .merge(rooms::rooms_router())
        .merge(players::players_router())
        .merge(transcode::transcode_router())
        .merge(v1_link_routes())
        .layer(middleware::from_fn(michi_security::content_type_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors_layer(&state))
        .with_state(state)
}

fn cors_layer(state: &AppState) -> CorsLayer {
    if state.config.dev_mode {
        return CorsLayer::permissive();
    }
    let methods = [axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::PUT, axum::http::Method::DELETE];
    let headers = [axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION];
    if let Some(ref origin) = state.config.cors_origin {
        match origin.parse::<axum::http::HeaderValue>() {
            Ok(header_origin) => CorsLayer::new()
                .allow_origin(tower_http::cors::AllowOrigin::exact(header_origin))
                .allow_methods(methods)
                .allow_headers(headers),
            Err(_) => {
                tracing::warn!("invalid MICHI_CORS_ORIGIN value, using restrictive CORS");
                CorsLayer::new()
            }
        }
    } else {
        CorsLayer::new()
    }
}
