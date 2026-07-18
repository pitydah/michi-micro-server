use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use tokio::sync::RwLock;
use tracing::{info, warn};

struct Watchdog {
    health: Arc<RwLock<Vec<WorkerHealth>>>,
}

#[derive(Clone)]
struct WorkerHealth {
    name: &'static str,
    last_heartbeat: Arc<RwLock<tokio::time::Instant>>,
}

impl WorkerHealth {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            last_heartbeat: Arc::new(RwLock::new(tokio::time::Instant::now())),
        }
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
                    let last = *w.last_heartbeat.read().await;
                    if now.duration_since(last) > Duration::from_secs(15) {
                        warn!(
                            "watchdog: worker '{}' last heartbeat {}s ago, may be hung",
                            w.name,
                            now.duration_since(last).as_secs()
                        );
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

    info!(
        version = %config.version(),
        port = %config.port(),
        music_path = %config.primary_music_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string()),
        database = %config.database_url,
        "starting Michi Micro Server",
    );

    let pool = michi_db::init_pool(&config.database_url).await?;

    let identity = michi_identity::MichiIdentity::load_or_create(&config.config_path).await?;
    info!("michi_id: {}...", &identity.get_id().await[..12]);

    let michi_connect = michi_connect::MichiConnect::new(
        identity.clone(),
        config.port(),
        Some("0.0.0.0".to_string()),
    );
    let _ = michi_connect.announce_mdns().await;

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
    let state = michi_api::AppState::new(config.clone(), pool, admin_user_id);
    let app = michi_api::create_router(state.clone());

    let os_router = michi_opensubsonic::routes::router(michi_opensubsonic::routes::OsAppState {
        db: state.db.clone(),
        music_paths: config.music_paths.clone(),
        cache_path: config.cache_path.clone(),
    });
    let app = app.merge(os_router);

    // Start sync peer connections (respeta módulo sync)
    michi_api::start_sync_peers(&state);

    // Start Home Assistant MQTT integration (respeta módulo homeassistant)
    if std::env::var("MICHI_MQTT_HOST").is_ok() {
        let ha_dm = state.disabled_modules.clone();
        if !ha_dm.read().await.contains("homeassistant") {
            let ha_config = config.clone();
            let ha_playback = state.playback_state.clone();
            let ha_db = state.db.clone();
            let ha_shutdown = state.shutdown_token.clone();
            let ha_cancel = state.homeassistant_cancel.clone();
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
    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Graceful shutdown: SIGINT + SIGTERM
    let shutdown_token = state.shutdown_token.clone();
    let shutdown_tx = state.tx.clone();
    let shutdown_db = state.db.clone();

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            use tokio::signal::unix::{signal, SignalKind};

            let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
            let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

            tokio::select! {
                _ = sigint.recv() => {
                    info!("received SIGINT, starting graceful shutdown...");
                }
                _ = sigterm.recv() => {
                    info!("received SIGTERM, starting graceful shutdown...");
                }
            }

            // 1. Cancel global shutdown token — all background tasks will stop
            shutdown_token.cancel();
            info!("background tasks notified of shutdown");

            // 2. Close WebSocket connections via broadcast
            let _ = shutdown_tx.send("shutdown".to_string());
            tokio::time::sleep(Duration::from_millis(500)).await;

            // 3. Checkpoint WAL
            let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(&shutdown_db).await;
            info!("WAL checkpoint complete");

            // 4. Allow brief time for tasks to finish
            tokio::time::sleep(Duration::from_secs(3)).await;
            info!("shutdown complete");
        })
        .await?;

    Ok(())
}
