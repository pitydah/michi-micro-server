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
        music_path = %config.music_path.display(),
        database = %config.database_url,
        "starting Michi Micro Server",
    );

    let pool = michi_db::init_pool(&config.database_url).await?;

    let state = michi_api::AppState::new(config.clone(), pool);

    let app = michi_api::create_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
