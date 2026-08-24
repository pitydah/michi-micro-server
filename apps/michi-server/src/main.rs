use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Starting,
    Running,
    Idle,
    Waiting,
    Blocked,
    Stopped,
    Failed,
}

struct Watchdog {
    health: Arc<RwLock<Vec<WorkerHealth>>>,
}

#[derive(Clone)]
struct WorkerHealth {
    name: &'static str,
    last_heartbeat: Arc<RwLock<tokio::time::Instant>>,
    state: Arc<RwLock<WorkerState>>,
}

impl WorkerHealth {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            last_heartbeat: Arc::new(RwLock::new(tokio::time::Instant::now())),
            state: Arc::new(RwLock::new(WorkerState::Idle)),
        }
    }

    #[allow(dead_code)]
    async fn set_state(&self, new_state: WorkerState) {
        *self.state.write().await = new_state;
        *self.last_heartbeat.write().await = tokio::time::Instant::now();
    }

    async fn tick(&self) {
        *self.last_heartbeat.write().await = tokio::time::Instant::now();
    }
}

impl Watchdog {
    fn new() -> Self {
        Self {
            health: Arc::new(RwLock::new(Vec::new())),
        }
    }

    async fn register(&self, name: &'static str) -> WorkerHealth {
        let wh = WorkerHealth::new(name);
        self.health.write().await.push(wh.clone());
        wh
    }

    async fn run(&self) {
        let health = self.health.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                let now = tokio::time::Instant::now();
                let workers = health.read().await;
                for w in workers.iter() {
                    let st = *w.state.read().await;
                    if st == WorkerState::Running {
                        let last = *w.last_heartbeat.read().await;
                        if now.duration_since(last) > Duration::from_secs(15) {
                            warn!(
                                "watchdog: worker '{}' in state {:?} last heartbeat {}s ago, may be hung",
                                w.name,
                                st,
                                now.duration_since(last).as_secs()
                            );
                        }
                    }
                }
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "michi=info,tower_http=info".into()),
        )
        .init();

    let config = michi_config::Config::from_env();

    // Guard: OpenSubsonic must not run without authentication.
    // An unauthenticated OpenSubsonic endpoint exposes the entire library to anyone on the network.
    if config.opensubsonic_enabled && !config.auth_enabled {
        let dev_bypass = std::env::var("MICHI_DEV_ALLOW_OPENSUBSONIC_NO_AUTH")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        if !dev_bypass {
            eprintln!(
                "FATAL: MICHI_OPENSUBSONIC_ENABLED=true requires authentication to be configured.\n\
                 Set MICHI_AUTH_USERNAME and MICHI_AUTH_PASSWORD before enabling OpenSubsonic.\n\
                 To bypass this check in development only, set MICHI_DEV_ALLOW_OPENSUBSONIC_NO_AUTH=true."
            );
            std::process::exit(1);
        }
        warn!("OpenSubsonic running without authentication — MICHI_DEV_ALLOW_OPENSUBSONIC_NO_AUTH override active. NOT safe for production.");
    }

    info!(
        version = %config.version(),
        port = %config.port(),
        music_path = %config.primary_music_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string()),
        database = %config.database_url,
        "starting Michi Micro Server",
    );

    let pool =
        michi_db::init_pool_with_size(&config.database_url, config.resource_profile.db_pool_size())
            .await?;

    let identity = Arc::new(michi_identity::IdentityManager::load_or_generate(
        &config.config_path,
        "Michi Micro Server",
        "",
    )?);
    info!("michi_id: {}...", &identity.michi_id().to_base64url()[..12]);

    let watchdog = Watchdog::new();
    watchdog.run().await;

    let _sync_health = watchdog.register("sync_peer").await;
    let _ingest_health = watchdog.register("ingest").await;
    let _playback_health = watchdog.register("playback").await;

    let sync_h = _sync_health.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            sync_h.tick().await;
        }
    });

    let admin_user_id = michi_api::init_admin_user(&config, &pool).await;
    let state = michi_api::AppState::new_with_identity(
        config.clone(),
        pool,
        admin_user_id,
        identity.clone(),
    );
    let app = michi_api::create_router(state.clone());

    let app = if config.opensubsonic_enabled {
        let os_router =
            michi_opensubsonic::routes::router(michi_opensubsonic::routes::OsAppState {
                db: state.db.clone(),
                music_paths: config.music_paths.clone(),
                cache_path: config.cache_path.clone(),
                auth_username: config.auth_username.clone(),
                auth_password: config.auth_password.clone(),
                auth_enabled: config.auth_enabled,
            });
        info!("OpenSubsonic compatibility API enabled at /rest/*");
        app.merge(os_router)
    } else {
        info!("OpenSubsonic compatibility API disabled (set MICHI_OPENSUBSONIC_ENABLED=true to enable)");
        app
    };

    // Start sync peer connections (respeta módulo sync)
    michi_api::start_sync_peers(&state);

    // Start Home Assistant MQTT integration (respeta módulo homeassistant)
    if std::env::var("MICHI_MQTT_HOST").is_ok() {
        let ha_dm = state.disabled_modules.clone();
        if !ha_dm.read().await.contains("homeassistant") {
            let ha_config = config.clone();
            let ha_playback = state.playback_state.clone();
            let ha_db = state.db.clone();
            let _ha_shutdown = state.shutdown_token.clone();
            let ha_cancel = state
                .module_tokens
                .try_read()
                .ok()
                .and_then(|m| m.get("homeassistant").cloned())
                .unwrap_or_default();
            tokio::spawn(async move {
                tokio::select! {
                    _ = ha_cancel.cancelled() => {
                        info!("homeassistant module cancelled, HA not started");
                    }
                    _ = async {
                        if ha_dm.read().await.contains("homeassistant") {
                            info!("homeassistant module disabled at startup");
                            futures_util::future::pending::<()>().await;
                        }
                        michi_homeassistant::run(ha_config, ha_playback, ha_db).await;
                    } => {}
                }
                info!("homeassistant stopped");
            });
        } else {
            info!("homeassistant module disabled, not starting");
        }
    } else {
        info!("MICHI_MQTT_HOST not set, Home Assistant integration disabled");
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();
    info!("listening on http://0.0.0.0:{}", actual_port);

    let advertised_host = michi_connect::MichiConnect::resolve_lan_ip();
    info!("advertised LAN host: {}", advertised_host);

    let michi_connect = michi_connect::MichiConnect::new(
        identity.clone(),
        actual_port,
        Some(advertised_host.clone()),
    );
    let _ = michi_connect.announce_mdns().await;

    // Spawn UDP multicast discovery announcer with cancellation on shutdown
    let announcer_cancel = tokio_util::sync::CancellationToken::new();
    let discovery_features = michi_api::server_caps::ServerCapabilities::from_state(&state)
        .await
        .discovery_features();
    let announcer_handle = michi_connect.spawn_announcer(
        actual_port,
        Some(advertised_host),
        discovery_features,
        announcer_cancel.clone(),
    );

    // Graceful shutdown: SIGINT + SIGTERM
    let shutdown_state = state.clone();
    let shutdown_tx = state.tx.clone();
    let shutdown_db = state.db.clone();

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = sigint.recv() => {
                info!("received SIGINT, starting graceful shutdown...");
            }
            _ = sigterm.recv() => {
                info!("received SIGTERM, starting graceful shutdown...");
            }
        }

        announcer_cancel.cancel();
        let _ = announcer_handle.await;
        michi_connect.stop_mdns().await;

        shutdown_state
            .shutdown_and_wait(Duration::from_secs(15))
            .await;

        let _ = shutdown_tx.send("shutdown".to_string());
        tokio::time::sleep(Duration::from_millis(500)).await;

        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&shutdown_db)
            .await;
        info!("WAL checkpoint complete");

        info!("shutdown complete");
    })
    .await?;

    Ok(())
}
