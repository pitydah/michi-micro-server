use std::net::SocketAddr;

use anyhow::Result;
use tracing::info;

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

    let admin_user_id = michi_api::init_admin_user(&config, &pool).await;
    let state = michi_api::AppState::new(config.clone(), pool, admin_user_id);
    let app = michi_api::create_router(state.clone());

    // Start sync peer connections in background
    michi_api::start_sync_peers(&state);

    // Start Home Assistant MQTT integration if env vars are set
    if std::env::var("MICHI_MQTT_HOST").is_ok() {
        let ha_config = config.clone();
        let ha_playback = state.playback_state.clone();
        let ha_db = state.db.clone();
        tokio::spawn(async move {
            michi_homeassistant::run(ha_config, ha_playback, ha_db).await;
        });
    } else {
        info!("MICHI_MQTT_HOST not set, Home Assistant integration disabled");
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
