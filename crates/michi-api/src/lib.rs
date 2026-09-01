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
use michi_playback::TrackResolver;
use michi_security::SecurityState;
use michi_sync::PlaybackState;
use michi_sync::SyncManager;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
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
pub mod server_caps;
mod static_files;
mod status;
mod stream;
mod sync_api;
mod sync_ws;
mod transcode;
mod ws;

pub mod output;
pub use output::{PlaybackOutputSelection, ResolvedOutputPlan};
pub mod playback_projection;
pub use playback_projection::PlaybackProjectionHealth;
pub mod playback_queue;
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
    pub module_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
    pub job_cancel_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
    pub task_handles: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// Canonical server identity (Ed25519 + BLAKE3), shared with michi-connect.
    pub identity: Arc<michi_identity::IdentityManager>,
    /// Canonical v1-lite pairing registry (RAM-only sessions, rate limited).
    pub pairing_registry: Arc<michi_identity::PairingRegistry>,
    /// Testability observer for generated pairing PINs without leaking over network.
    pub pairing_display: Arc<RwLock<Option<String>>>,
    /// Testability observer mapping session_id -> PIN for safe concurrent pairing.
    pub pairing_sessions_display: Arc<RwLock<HashMap<String, String>>>,
    /// Real autonomous playback engine handle.
    pub playback_engine: michi_playback::PlaybackEngineHandle,
    /// Playback projection coordinator for authorative single-writer persistence.
    pub playback_projection: playback_projection::PlaybackProjectionCoordinator,
    /// Observable health metrics of the playback projection coordinator.
    pub playback_projection_health: Arc<RwLock<PlaybackProjectionHealth>>,
    /// Explicit output target selection (Receiver, RoomGroup, or Chain).
    pub playback_output_selection: Arc<RwLock<Option<output::PlaybackOutputSelection>>>,
    /// Encrypted credential store for paired receivers (None if persistent key unavailable).
    pub receiver_credential_store: Arc<Option<michi_receivers::ReceiverCredentialStore>>,
    /// Resource-bounded transcode semaphore derived from ResourceProfile.max_transcodes.
    pub transcode_semaphore: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    pub fn track_task(&self, handle: tokio::task::JoinHandle<()>) {
        self.task_handles.lock().unwrap().push(handle);
    }

    pub async fn shutdown_and_wait(&self, timeout: Duration) {
        // 1. Explicit bounded projection flush while PlaybackEngine is active
        let flush_timeout = Duration::from_millis(1500).min(timeout);
        if let Err(e) =
            tokio::time::timeout(flush_timeout, self.playback_projection.flush_shutdown()).await
        {
            tracing::warn!("shutdown playback projection flush timed out: {e:?}");
        }
        // 2. Shutdown PlaybackEngine cleanly
        self.playback_engine.shutdown().await;
        // 3. Cancel remaining background tasks
        self.shutdown_token.cancel();
        // 4. Join all tracked tasks
        let handles: Vec<tokio::task::JoinHandle<()>> =
            std::mem::take(&mut *self.task_handles.lock().unwrap());
        for handle in handles {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
        // 5. Explicit WAL checkpoint
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.db)
            .await;
    }

    pub async fn bootstrap_runtime(&self) -> Result<(), String> {
        tracing::info!(
            "bootstrapping runtime: loading persisted receivers and restoring credentials"
        );
        match michi_db::list_receivers_db(&self.db).await {
            Ok(persisted_list) => {
                let registry_arc = self.receiver_manager.registry().await;
                let mut registry = registry_arc.write().await;
                let mut restored_count = 0;

                for rec in persisted_list {
                    let mut token: Option<String> = None;
                    if rec.paired {
                        if let Some(store) = self.receiver_credential_store.as_ref() {
                            match michi_db::get_receiver_credential_db(&self.db, &rec.id).await {
                                Ok(Some(cred)) => {
                                    match store.decrypt_token(
                                        &rec.id,
                                        &cred.ciphertext,
                                        &cred.nonce,
                                    ) {
                                        Ok(decrypted) => {
                                            token = Some(decrypted);
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "failed to decrypt credential for receiver {}: {} (marking unpaired/fail-closed)",
                                                rec.id,
                                                e
                                            );
                                        }
                                    }
                                }
                                Ok(None) => {
                                    tracing::warn!(
                                        "no encrypted credential record found for paired receiver {}",
                                        rec.id
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "error querying credentials for receiver {}: {}",
                                        rec.id,
                                        e
                                    );
                                }
                            }
                        } else {
                            tracing::warn!(
                                "receiver credential store unavailable, cannot restore token for receiver {}",
                                rec.id
                            );
                        }
                    }

                    let is_paired = rec.paired && token.is_some();
                    let entry = michi_receivers::ReceiverRegistryEntry {
                        receiver_id: rec.id.clone(),
                        name: rec.name,
                        device_type: rec.device_type,
                        base_url: rec.base_url,
                        paired: is_paired,
                        token,
                        last_seen: rec.last_seen.and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s)
                                .ok()
                                .map(|d| d.with_timezone(&chrono::Utc))
                        }),
                        capabilities: vec!["pcm".to_string(), "rtp".to_string()],
                        active_session_id: None,
                        max_sample_rate: 48000,
                        max_bit_depth: 16,
                        supported_transports: vec!["rtp".to_string()],
                        supported_codecs: vec!["pcm_s16le".to_string()],
                        supported_sample_rates: vec![44100, 48000],
                        supported_bit_depths: vec![16],
                        supported_channels: vec![2],
                        maximum_safe_volume: Some(100),
                    };

                    registry.add(entry);
                    restored_count += 1;
                }
                tracing::info!(
                    "restored {} persisted receivers into registry",
                    restored_count
                );
            }
            Err(e) => {
                tracing::warn!("failed to list persisted receivers from DB: {}", e);
            }
        }

        tracing::info!("spawning sync upload startup crash recovery scan in background");
        let sync_mgr = self.sync_manager.clone();
        let shutdown_tok = self.shutdown_token.clone();
        self.track_task(tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_tok.cancelled() => {
                    tracing::info!("sync startup recovery aborted by shutdown");
                }
                res = sync_mgr.recover_incomplete_uploads() => {
                    match res {
                        Ok(report) => {
                            tracing::info!(
                                candidates = report.candidates,
                                completed = report.completed,
                                deferred = report.deferred,
                                terminal = report.terminal_failures,
                                transient = report.transient_failures,
                                invalid = report.invalid_rows,
                                "sync upload startup crash recovery completed"
                            );
                        }
                        Err(e) => {
                            tracing::warn!("sync upload startup recovery scan encountered error: {}", e);
                        }
                    }
                }
            }
        }));

        Ok(())
    }
}

impl AppState {
    pub fn new(config: Config, db: SqlitePool, admin_user_id: Option<Uuid>) -> Self {
        let identity = Arc::new(
            michi_identity::IdentityManager::load_or_generate(
                &config.config_path,
                "Michi Micro Server",
                "",
            )
            .unwrap_or_else(|e| {
                tracing::warn!("failed to load identity from config dir: {e}; using ephemeral");
                let ephemeral_dir = std::env::temp_dir()
                    .join(format!("michi-ephemeral-identity-{}", uuid::Uuid::new_v4()));
                let _ = std::fs::create_dir_all(&ephemeral_dir);
                michi_identity::IdentityManager::generate(&ephemeral_dir, "Michi Micro Server", "")
                    .expect("ephemeral identity generation must not fail")
            }),
        );
        Self::new_with_identity(config, db, admin_user_id, identity)
    }

    pub fn new_with_identity(
        config: Config,
        db: SqlitePool,
        admin_user_id: Option<Uuid>,
        identity: Arc<michi_identity::IdentityManager>,
    ) -> Self {
        let (tx, _) = broadcast::channel(64);
        let (sync_tx, _) = broadcast::channel(64);
        let auth_sessions = auth::AuthState::new();
        let auth_enabled = config.auth_enabled;
        if auth_enabled {
            auth::spawn_session_cleanup(auth_sessions.clone());
        }
        let token_store = michi_link::TokenStore::new();
        let playback_state = Arc::new(RwLock::new(PlaybackState::default()));
        let upload_dir = config.cache_path.join("uploads");
        let _ = std::fs::create_dir_all(&upload_dir);
        let sync_manager = michi_sync::SyncManager::new(db.clone(), upload_dir);
        let security_config = michi_security::SecurityConfig::default();
        let security_state = michi_security::SecurityState::new(security_config);

        let disabled_modules: Arc<RwLock<HashSet<String>>> = Arc::new(RwLock::new(HashSet::new()));
        let shutdown_token = CancellationToken::new();

        let mut module_tokens = HashMap::new();
        module_tokens.insert("scan".to_string(), CancellationToken::new());
        module_tokens.insert("sync".to_string(), CancellationToken::new());
        module_tokens.insert("playback".to_string(), CancellationToken::new());
        module_tokens.insert("backup".to_string(), CancellationToken::new());
        module_tokens.insert("webhook".to_string(), CancellationToken::new());
        module_tokens.insert("homeassistant".to_string(), CancellationToken::new());
        let module_tokens = Arc::new(RwLock::new(module_tokens));
        let job_cancel_tokens: Arc<RwLock<HashMap<String, CancellationToken>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let task_handles: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

        let db_for_tokens = db.clone();
        let ts = token_store.clone();
        let th = task_handles.clone();
        tokio::spawn(async move {
            match michi_link::load_tokens_from_db(&ts, &db_for_tokens).await {
                Ok(n) => tracing::info!("loaded {} device tokens from DB", n),
                Err(e) => tracing::warn!("failed to load device tokens from DB: {}", e),
            }
            th.lock().unwrap().retain(|h| !h.is_finished());
        });
        michi_link::spawn_token_cleanup(token_store.clone());

        let receiver_manager =
            michi_receivers::ReceiverSessionManager::new_with_identity(identity.clone());
        let pairing_registry = Arc::new(michi_identity::PairingRegistry::new());
        let pairing_display = Arc::new(RwLock::new(None));
        let pairing_sessions_display = Arc::new(RwLock::new(HashMap::new()));

        let cred_key_path = config.config_path.join("receiver_credentials.key");
        let receiver_credential_store = Arc::new(
            michi_receivers::ReceiverCredentialStore::load_or_create_key(&cred_key_path)
                .map_err(|e| {
                    tracing::warn!(
                        "failed to load receiver credentials key from {:?}: {}",
                        cred_key_path,
                        e
                    );
                    e
                })
                .ok(),
        );

        let resolver = Arc::new(michi_playback::SqliteTrackResolver::new(
            db.clone(),
            config.music_paths.clone(),
        ));
        let (playback_engine, engine_join) =
            michi_playback::spawn_playback_engine(resolver, michi_playback::PcmFormat::default());
        task_handles.lock().unwrap().push(engine_join);

        let playback_output_selection = Arc::new(RwLock::new(None));

        let (playback_projector, playback_projection_health) =
            playback_projection::PlaybackProjectionCoordinator::new(
                db.clone(),
                playback_state.clone(),
                playback_engine.clone(),
            );
        let projector_handle = playback_projector.clone().spawn(shutdown_token.clone());
        task_handles.lock().unwrap().push(projector_handle);

        let max_transcodes = config.resource_profile.max_transcodes();
        let transcode_semaphore = Arc::new(tokio::sync::Semaphore::new(max_transcodes));

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
            module_tokens,
            job_cancel_tokens,
            task_handles,
            identity,
            pairing_registry,
            pairing_display,
            pairing_sessions_display,
            playback_engine,
            playback_projection: playback_projector,
            playback_projection_health,
            playback_output_selection,
            receiver_credential_store,
            transcode_semaphore,
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
        routes::v1::playback::auto_restore_playback_state(
            db.clone(),
            self.playback_state.clone(),
            self.playback_engine.clone(),
            self.config.music_paths.clone(),
        );

        // DB maintenance scheduler (siempre corre)
        let maintenance_db = db.clone();
        let maint_shutdown = shutdown.clone();
        self.track_task(tokio::spawn(async move {
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
                        if let Err(e) = michi_db::run_hourly_maintenance(&maintenance_db).await {
                            tracing::warn!(error = %e, "hourly database maintenance failed");
                        }
                    }
                    _ = daily.tick() => {
                        if let Err(e) = michi_db::run_daily_maintenance(&maintenance_db).await {
                            tracing::warn!(error = %e, "daily database maintenance failed");
                        }
                    }
                    _ = weekly.tick() => {
                        if let Err(e) = michi_db::run_weekly_maintenance(&maintenance_db).await {
                            tracing::warn!(error = %e, "weekly database maintenance failed");
                        }
                    }
                }
            }
            info!("maintenance scheduler stopped");
        }));

        // Periodic rate limiter & pairing attempt cache pruner (runs every 60s)
        let sec_state_for_prune = self.security_state.clone();
        let sec_prune_shutdown = shutdown.clone();
        self.track_task(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = sec_prune_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        sec_state_for_prune.prune_stale_limiters();
                    }
                }
            }
        }));

        // Daily library integrity & reconciliation cron (solo si scan habilitado)

        let integrity_db = db.clone();
        let integrity_paths = self.config.music_paths.clone();
        let integrity_profile = self.config.resource_profile;
        let integrity_shutdown = shutdown.clone();
        let integrity_dm = dm.clone();
        let integrity_tokens = self.module_tokens.clone();
        self.track_task(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(86400));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = integrity_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        if integrity_dm.read().await.contains("scan") {
                            continue;
                        }
                        let current_cancel = integrity_tokens
                            .read()
                            .await
                            .get("scan")
                            .cloned()
                            .unwrap_or_default();
                        if current_cancel.is_cancelled() {
                            continue;
                        }
                        let _concurrency = integrity_profile.scan_concurrency();
                        for path in &integrity_paths {
                            if current_cancel.is_cancelled() {
                                break;
                            }
                            let scan_res = michi_scanner::scan_root_cancellable(
                                path,
                                current_cancel.clone(),
                            )
                            .await;
                            let _ = michi_scanner::reconcile_root(
                                &integrity_db,
                                path,
                                &scan_res,
                                &current_cancel,
                            )
                            .await;
                        }
                    }
                }
            }
            info!("integrity cron stopped");
        }));

        // Inotify file system watcher (respeta módulo scan con re-habilitación dinámica)
        let watch_paths = self.config.music_paths.clone();
        let watch_db = db.clone();
        let watch_shutdown = shutdown.clone();
        let watch_dm = dm.clone();
        let watch_tokens = self.module_tokens.clone();
        self.track_task(tokio::spawn(async move {
            if !watch_paths.is_empty() {
                let watcher = michi_scanner::watcher::LibraryWatcher::new(
                    watch_paths.clone(),
                    watch_db.clone(),
                );
                let mut was_disabled = false;
                loop {
                    if watch_shutdown.is_cancelled() {
                        break;
                    }
                    if watch_dm.read().await.contains("scan") {
                        was_disabled = true;
                        tokio::select! {
                            _ = watch_shutdown.cancelled() => break,
                            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                        }
                    }
                    let current_cancel = watch_tokens
                        .read()
                        .await
                        .get("scan")
                        .cloned()
                        .unwrap_or_default();
                    if current_cancel.is_cancelled() {
                        was_disabled = true;
                        tokio::select! {
                            _ = watch_shutdown.cancelled() => break,
                            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                        }
                    } else {
                        if was_disabled {
                            info!(
                                "scan module re-enabled; reconciling library roots with filesystem"
                            );
                            for path in &watch_paths {
                                if current_cancel.is_cancelled() {
                                    break;
                                }
                                let scan_res = michi_scanner::scan_root_cancellable(
                                    path,
                                    current_cancel.clone(),
                                )
                                .await;
                                let _ = michi_scanner::reconcile_root(
                                    &watch_db,
                                    path,
                                    &scan_res,
                                    &current_cancel,
                                )
                                .await;
                            }
                            was_disabled = false;
                        }
                        watcher
                            .run(
                                current_cancel,
                                watch_shutdown.clone(),
                                Duration::from_secs(5),
                            )
                            .await;
                    }
                }
            }
            info!("watcher stopped");
        }));

        // Job Queue supervisor
        let supervisor_db = db.clone();
        let supervisor_state = self.clone();
        let supervisor_shutdown = shutdown.clone();
        let supervisor_dm = dm.clone();
        self.track_task(tokio::spawn(async move {
            let max_jobs = (supervisor_state.config.job_max_concurrent as usize).max(1);
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
                            let permit = match tokio::select! {
                                _ = supervisor_shutdown.cancelled() => None,
                                result = semaphore.clone().acquire_owned() => result.ok(),
                            } {
                                Some(permit) => permit,
                                None => break,
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
                            let cancel = supervisor_shutdown.child_token();
                            supervisor_state
                                .job_cancel_tokens
                                .write()
                                .await
                                .insert(job_id.clone(), cancel.clone());

                            tokio::spawn(async move {
                                let _permit = permit;
                                let result = run_job_worker(
                                    &worker_db,
                                    &worker_state,
                                    &job_id,
                                    &job_kind,
                                    job_payload.as_ref(),
                                    &worker_dm,
                                    &cancel,
                                )
                                .await;
                                if cancel.is_cancelled() {
                                    tracing::info!("job {} cancelled", job_id);
                                } else {
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
                                }
                                worker_state.job_cancel_tokens.write().await.remove(&job_id);
                            });
                        }
                    }
                }
            }
            info!("job supervisor stopped");
        }));

        // Auto-Backup Scheduler
        if self.config.auto_backup_enabled {
            let backup_state = self.clone();
            let backup_shutdown = shutdown.clone();
            let backup_dm = dm.clone();
            self.track_task(tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(3600));
                loop {
                    tokio::select! {
                        _ = backup_shutdown.cancelled() => break,
                        _ = interval.tick() => {
                            if backup_dm.read().await.contains("backup") {
                                continue;
                            }
                            if let Err(e) = routes::v1::backup::run_auto_backup_cycle(&backup_state).await {
                                tracing::warn!("auto-backup cycle encountered error: {}", e);
                            }
                        }
                    }
                }
                info!("auto-backup scheduler stopped");
            }));
        }

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

/// Get the current CancellationToken for a module (supports dynamic replacement)
impl AppState {
    pub async fn scan_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("scan")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn sync_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("sync")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn playback_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("playback")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn backup_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("backup")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn webhook_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("webhook")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
    pub async fn homeassistant_token(&self) -> CancellationToken {
        self.module_tokens
            .read()
            .await
            .get("homeassistant")
            .cloned()
            .unwrap_or_else(CancellationToken::new)
    }
}

async fn run_job_worker(
    db: &SqlitePool,
    state: &AppState,
    job_id: &str,
    kind: &str,
    _payload: Option<&serde_json::Value>,
    _dm: &Arc<RwLock<HashSet<String>>>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    if cancel.is_cancelled() {
        return Err("cancelled".into());
    }
    match kind {
        "scan" => {
            tracing::info!("job {}: starting library scan", job_id);
            let paths = &state.config.music_paths;
            let mut total = 0usize;
            for (index, path) in paths.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err("cancelled".into());
                }
                let probe_path = path.clone();
                let available = tokio::task::spawn_blocking(move || {
                    std::fs::read_dir(probe_path)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| format!("mount probe failed: {error}"))?;
                if let Err(error) = available {
                    let _ = michi_db::update_mount_state(
                        db,
                        &path.display().to_string(),
                        "unavailable",
                        &error,
                    )
                    .await;
                    tracing::warn!(path = %path.display(), %error, "scan skipped unavailable mount");
                    continue;
                }
                let scan_res = michi_scanner::scan_root_cancellable(path, cancel.clone()).await;
                michi_scanner::reconcile_root(db, path, &scan_res, cancel)
                    .await
                    .map_err(|error| format!("reconcile error: {error}"))?;
                total += scan_res.tracks().len();
                let progress = (index + 1) as f64 / paths.len().max(1) as f64;
                let _ = michi_db::update_job_progress(db, job_id, progress).await;
            }
            record_audit(db, "scan_completed", Some("library"), None, None).await;
            Ok(format!("scanned {total} tracks"))
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
            if cancel.is_cancelled() {
                return Err("cancelled".into());
            }
            tracing::info!("job {}: running backup", job_id);
            let tracks = michi_db::list_tracks(db)
                .await
                .map_err(|e| format!("list tracks error: {e}"))?;
            let playlists = michi_db::list_playlists(db, None)
                .await
                .map_err(|e| format!("list playlists error: {e}"))?;
            let output = serde_json::json!({
                "exported_at": Utc::now().to_rfc3339(),
                "tracks_count": tracks.len(),
                "playlists_count": playlists.len(),
            });
            record_audit(db, "backup_completed", Some("backup"), None, Some(output)).await;
            let _ = michi_db::update_job_progress(db, job_id, 1.0).await;
            Ok(format!(
                "backup complete: {} tracks, {} playlists",
                tracks.len(),
                playlists.len()
            ))
        }
        "cleanup" => {
            if cancel.is_cancelled() {
                return Err("cancelled".into());
            }
            tracing::info!("job {}: running cleanup", job_id);
            michi_db::run_hourly_maintenance(db)
                .await
                .map_err(|e| format!("cleanup error: {e}"))?;
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(db)
                .await
                .map_err(|e| format!("checkpoint error: {e}"))?;
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
        _ => Err(format!("unknown job kind: {kind}")),
    }
}

pub async fn init_admin_user(config: &Config, db: &SqlitePool) -> Option<Uuid> {
    if !config.auth_enabled {
        return None;
    }
    let username = config
        .auth_username
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())?;
    let password = config
        .auth_password
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| s.len() >= 8)?;

    match michi_db::get_user_by_username(db, username)
        .await
        .ok()
        .flatten()
    {
        Some((id, _, _, is_admin)) => {
            if !is_admin {
                if sqlx::query("UPDATE users SET is_admin = 1 WHERE id = ?")
                    .bind(id.to_string())
                    .execute(db)
                    .await
                    .is_err()
                {
                    warn!("failed to promote configured admin user");
                    return None;
                }
                info!("promoted configured user '{}' to administrator", username);
            }
            Some(id)
        }
        None => {
            let id = Uuid::new_v4();
            match auth::hash_password(password) {
                Ok(hash) => match michi_db::create_user(db, &id, username, &hash, true).await {
                    Ok(_) => {
                        info!("created admin user: {}", username);
                        Some(id)
                    }
                    Err(e) => {
                        warn!("failed to create admin user: {e}");
                        None
                    }
                },
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
    let engine = state.playback_engine.clone();
    let db = state.db.clone();
    let music_paths = state.config.music_paths.clone();
    let reconnect_max = state.config.reconnect_delay_max as u64;
    let shutdown = state.shutdown_token.clone();
    let dm = state.disabled_modules.clone();
    let sync_cancel = state
        .module_tokens
        .try_read()
        .ok()
        .and_then(|m| m.get("sync").cloned())
        .unwrap_or_default();

    state.track_task(tokio::spawn(async move {
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
                    let peer_engine = engine.clone();
                    let peer_db = db.clone();
                    let peer_music_paths = music_paths.clone();
                    let peer_shutdown = shutdown.clone();
                    let peer_dm = dm.clone();

                    tokio::spawn(async move {
                        let url = format!("ws://{peer}/api/sync");
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
                                            let recv_engine = peer_engine.clone();
                                            let recv_db = peer_db.clone();
                                            let recv_paths = peer_music_paths.clone();
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
                                                                if let Some(tid) = track_id {
                                                                    let resolver = michi_playback::SqliteTrackResolver::new(
                                                                        recv_db.clone(),
                                                                        recv_paths.clone(),
                                                                    );
                                                                    if let Ok(track) = resolver.get_track(tid).await {
                                                                        let _ = recv_engine.load_track(track, position_ms).await;
                                                                    }
                                                                } else {
                                                                    let _ = recv_engine.seek(position_ms).await;
                                                                }

                                                                if playing {
                                                                    let _ = recv_engine.resume().await;
                                                                } else {
                                                                    let _ = recv_engine.pause().await;
                                                                }
                                                                let vol_u8 = ((volume * 100.0).round().clamp(0.0, 100.0)) as u8;
                                                                let _ = recv_engine.set_volume(vol_u8).await;

                                                                let tid = track_id
                                                                    .map(|id| format!("\"{id}\""))
                                                                    .unwrap_or_else(|| "null".into());
                                                                let msg = format!(
                                                                    "{{\"type\":\"sync_state\",\
                                                                     \"track_id\":{tid},\
                                                                     \"position_ms\":{position_ms},\
                                                                     \"playing\":{playing},\
                                                                     \"volume\":{volume}}}",
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
                                    let delay = michi_config::Config::compute_reconnect_backoff(attempt, reconnect_max);
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
    }));
}
fn v1_link_routes() -> Router<AppState> {
    let api_v1 = Router::new()
        .route(
            "/api/v1/pair/qr",
            post(routes::v1::pair::qr_generate_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/status",
            get(routes::v1::pair::qr_status_handler),
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
        .route(
            "/api/v1/tracks/:id/stream",
            get(routes::v1::stream::stream_handler),
        )
        .route(
            "/api/v1/tracks/:id/download",
            get(routes::v1::stream::download_handler),
        )
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
            "/api/v1/playlists/:id/tracks",
            get(routes::v1::playlists::get_playlist_tracks_handler),
        )
        .route(
            "/api/v1/playlists/:playlist_id/tracks/:track_id",
            post(routes::v1::playlists::add_playlist_track_handler)
                .delete(routes::v1::playlists::remove_playlist_track_handler),
        )
        .route(
            "/api/v1/playlists/:id/reorder",
            put(routes::v1::playlists::reorder_playlist_tracks_handler),
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
            post(routes::v1::sync::sync_upload_chunk_handler)
                .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route(
            "/api/v1/sync/upload/:file_id/status",
            get(routes::v1::sync::sync_upload_status_handler),
        )
        .route(
            "/api/v1/sync/upload/file",
            post(routes::v1::sync::sync_upload_file_handler)
                .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024)),
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
            "/api/v1/import/session/:session_id/upload",
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
            "/api/v1/backup/restore",
            post(routes::v1::backup::restore_handler),
        )
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
            get(routes::v1::backup::backup_bundle_handler)
                .post(routes::v1::backup::restore_backup_bundle_handler),
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
            get(routes::v1::backup::verify_integrity_handler)
                .post(routes::v1::backup::verify_integrity_handler),
        )
        .route(
            "/api/v1/backup/verify",
            get(routes::v1::backup::verify_integrity_handler)
                .post(routes::v1::backup::verify_integrity_handler),
        )
        .route(
            "/api/v1/backup/download",
            get(routes::v1::backup::download_backup_handler),
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
        .route("/api/v1/modules", get(routes::v1::modules::modules_handler))
        .route(
            "/api/v1/modules/:name",
            post(routes::v1::modules::toggle_module_handler),
        )
        .route(
            "/api/v1/changes",
            get(routes::v1::modules::change_journal_handler),
        )
        .route(
            "/api/v1/stream/handoff/offer",
            post(routes::v1::modules::handoff_handler),
        )
        .route(
            "/api/v1/jobs",
            get(routes::v1::jobs::list_jobs_handler).post(routes::v1::jobs::create_job_handler),
        )
        .route("/api/v1/jobs/:id", get(routes::v1::jobs::get_job_handler))
        .route(
            "/api/v1/jobs/:id/cancel",
            post(routes::v1::jobs::cancel_job_handler),
        )
        .route(
            "/api/v1/playback/state",
            get(routes::v1::playback::playback_state_handler),
        )
        .route(
            "/api/v1/playback/output",
            get(routes::v1::playback::get_playback_output_handler)
                .put(routes::v1::playback::set_playback_output_handler)
                .post(routes::v1::playback::set_playback_output_handler),
        )
        .route(
            "/api/v1/playback/control",
            post(routes::v1::playback::playback_control_handler),
        )
        .route(
            "/api/v1/playback/seek",
            post(routes::v1::playback::playback_seek_handler),
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
            "/api/v1/playback/restore",
            post(routes::v1::playback::playback_session_restore_handler),
        )
        .route(
            "/api/v1/player/handoff",
            post(routes::v1::playback::handoff_handler),
        )
        .route(
            "/api/v1/playback/handoff",
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
            "/api/v1/receivers/pair/start",
            post(routes::v1::receivers::receiver_pair_start_handler),
        )
        .route(
            "/api/v1/receivers/pair/confirm",
            post(routes::v1::receivers::receiver_pair_confirm_handler),
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
        );

    // /stream/test_pcm is an engineering verification route, disabled in release builds
    let api_v1 = if cfg!(debug_assertions) || cfg!(test) {
        api_v1.route(
            "/api/v1/receivers/:id/stream/test_pcm",
            post(routes::v1::receivers::receiver_stream_test_pcm_handler),
        )
    } else {
        api_v1
    };

    api_v1
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
}

fn v1_public_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/server/info",
            get(routes::v1::server::server_info_handler),
        )
        .route("/api/v1/status", get(routes::v1::server::status_handler))
        .route("/health/live", get(routes::v1::server::health_live_handler))
        .route(
            "/health/ready",
            get(routes::v1::server::health_ready_handler),
        )
        .route(
            "/api/v1/capabilities",
            get(routes::v1::modules::capabilities_handler),
        )
        .route("/api/v1/policy", get(routes::v1::modules::policy_handler))
        .route(
            "/api/v1/policy/lan",
            post(routes::v1::modules::lan_policy_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/claim",
            post(routes::v1::pair::qr_claim_handler),
        )
        .route(
            "/api/v1/pair/qr/:qr_code/svg",
            get(routes::v1::pair::qr_svg_handler),
        )
        .route(
            "/api/v1/pair/confirm",
            post(routes::v1::pair::link_pair_confirm),
        )
        .route(
            "/api/v1/token/refresh",
            post(routes::v1::pair::link_token_refresh),
        )
}

fn v1_link_routes_with_auth(state: AppState) -> Router<AppState> {
    v1_link_routes()
        .route(
            "/api/v1/pair/start",
            post(routes::v1::pair::link_pair_start),
        )
        .route("/api/v1/events", get(routes::v1::events::events_handler))
        .route(
            "/api/v1/events/sse",
            get(routes::v1::events::events_sse_handler),
        )
        .route(
            "/api/v1/health/self-test",
            get(routes::v1::modules::self_test_handler),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::v1_auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.security_state.clone(),
            michi_security::rate_limit_middleware,
        ))
        .layer(middleware::from_fn(
            michi_security::security_headers_middleware,
        ))
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
        .route("/static/hero-cat.css", get(static_files::hero_cat_css))
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
        .route(
            "/static/assets/michi-micro-server-180.png",
            get(static_files::icon_180_png),
        )
        .route(
            "/static/assets/michi-micro-server-192.png",
            get(static_files::icon_192_png),
        )
        .route(
            "/static/assets/michi-micro-server-512.png",
            get(static_files::icon_512_png),
        )
        .route(
            "/static/assets/michi-hero-cat.webp",
            get(static_files::hero_cat_webp),
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
        .merge(
            sync_api::sync_router().layer(middleware::from_fn_with_state(
                state.clone(),
                auth::v1_auth_middleware,
            )),
        )
        .merge(rooms::rooms_router().layer(middleware::from_fn_with_state(
            state.clone(),
            auth::v1_auth_middleware,
        )))
        .merge(
            players::players_router().layer(middleware::from_fn_with_state(
                state.clone(),
                auth::v1_auth_middleware,
            )),
        )
        .merge(
            transcode::transcode_router().layer(middleware::from_fn_with_state(
                state.clone(),
                auth::v1_auth_middleware,
            )),
        )
        .merge(v1_link_routes_with_auth(state.clone()))
        .merge(v1_public_routes())
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
    let methods = [
        axum::http::Method::GET,
        axum::http::Method::POST,
        axum::http::Method::PUT,
        axum::http::Method::DELETE,
    ];
    let headers = [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::AUTHORIZATION,
    ];
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

pub fn resolve_client_ip(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: &axum::http::HeaderMap,
    config: &michi_config::Config,
) -> Option<std::net::IpAddr> {
    let peer_addr = connect_info.map(|axum::extract::ConnectInfo(addr)| addr);
    let peer_ip = peer_addr.map(|a| a.ip())?;

    if config.is_trusted_proxy(&peer_ip) {
        // Parse X-Forwarded-For from right to left (nearest proxy hop to furthest)
        if let Some(forwarded_header) = headers.get("X-Forwarded-For").and_then(|v| v.to_str().ok())
        {
            let mut resolved = None;
            for token in forwarded_header.split(',').map(|s| s.trim()).rev() {
                if let Ok(ip) = token.parse::<std::net::IpAddr>() {
                    if config.is_trusted_proxy(&ip) {
                        continue; // Skip intermediate trusted proxy hop
                    }
                    resolved = Some(ip);
                    break;
                }
            }
            if let Some(ip) = resolved {
                return Some(ip);
            }
        }

        // Fallback to X-Real-IP if present and valid
        if let Some(real_ip_header) = headers.get("X-Real-IP").and_then(|v| v.to_str().ok()) {
            if let Ok(ip) = real_ip_header.trim().parse::<std::net::IpAddr>() {
                return Some(ip);
            }
        }

        // The TCP peer is a trusted proxy, but no valid client IP could be extracted.
        // Fail-closed by returning None so we don't accidentally treat the proxy's private IP as the client.
        None
    } else {
        Some(peer_ip)
    }
}

pub fn extract_client_ip(
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: &axum::http::HeaderMap,
    config: &michi_config::Config,
) -> String {
    resolve_client_ip(connect_info, headers, config)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderMap, HeaderValue};
    use std::net::SocketAddr;

    fn make_test_config(trust_proxy: bool, trusted_proxies: Vec<&str>) -> michi_config::Config {
        michi_config::Config {
            port: 9090,
            music_paths: vec![std::path::PathBuf::from("/music")],
            config_path: std::path::PathBuf::from("/config"),
            cache_path: std::path::PathBuf::from("/cache"),
            database_url: "sqlite::memory:".to_string(),
            version: "test",
            sync_peers: Vec::new(),
            sync_name: "test".to_string(),
            listenbrainz_token: None,
            lastfm_token: None,
            scrobble_enabled: false,
            auth_username: None,
            auth_password: None,
            auth_enabled: false,
            allow_registration: false,
            server_id: uuid::Uuid::new_v4(),
            cors_origin: None,
            dev_mode: false,
            resource_profile: michi_core::ResourceProfile::from_config_str("eco"),
            stream_profile: michi_core::StreamProfile::from_config_str("original"),
            format_policy: michi_core::AudioFormatPolicy::from_config_str("lossless"),
            max_remote_bitrate: 320_000,
            remote_sync: false,
            language: "en".into(),
            ui: michi_config::UiConfig::default(),
            auto_backup_enabled: false,
            backup_max_keep: 7,
            job_max_concurrent: 3,
            reconnect_delay_max: 300,
            opensubsonic_enabled: false,
            trust_proxy,
            trusted_proxies: trusted_proxies
                .into_iter()
                .map(|s| s.parse().unwrap())
                .collect(),
        }
    }

    #[test]
    fn test_extract_client_ip_no_trust_proxy_ignores_headers() {
        let cfg = make_test_config(false, vec!["127.0.0.1"]);
        let addr: SocketAddr = "192.168.1.100:54321".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("8.8.8.8"));
        headers.insert("X-Real-IP", HeaderValue::from_static("8.8.4.4"));

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "192.168.1.100");
    }

    #[test]
    fn test_extract_client_ip_untrusted_peer_cannot_spoof() {
        let cfg = make_test_config(true, vec!["127.0.0.1", "10.0.0.1"]);
        // Untrusted LAN attacker connecting directly
        let addr: SocketAddr = "192.168.1.50:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("1.1.1.1"));

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "192.168.1.50");
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_accepts_forwarded() {
        let cfg = make_test_config(true, vec!["127.0.0.1", "10.0.0.1"]);
        // Trusted proxy connecting
        let addr: SocketAddr = "10.0.0.1:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("203.0.113.195, 10.0.0.1"),
        );

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "203.0.113.195");
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_handles_spoofed_prefix_right_to_left() {
        let cfg = make_test_config(true, vec!["10.0.0.1"]);
        // Attacker sent XFF: 127.0.0.1, proxy appended actual remote client 203.0.113.195
        let addr: SocketAddr = "10.0.0.1:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("127.0.0.1, 203.0.113.195"),
        );

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "203.0.113.195", "Must resolve to the true client IP (203.0.113.195) rather than the spoofed prefix (127.0.0.1)");
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_accepts_real_ip() {
        let cfg = make_test_config(true, vec!["127.0.0.1"]);
        let addr: SocketAddr = "127.0.0.1:41234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Real-IP", HeaderValue::from_static("198.51.100.4"));

        let ip = extract_client_ip(Some(ConnectInfo(addr)), &headers, &cfg);
        assert_eq!(ip, "198.51.100.4");
    }
}
