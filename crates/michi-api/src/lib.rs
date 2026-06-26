use std::sync::Arc;

use axum::{
    http::HeaderMap,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use michi_config::Config;
use michi_sync::PlaybackState;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use utoipa::OpenApi;
use uuid::Uuid;

mod auth;
mod library;
mod openapi;
mod pwa;
mod root;
mod scrobble;
mod status;
mod stream;
mod sync_ws;
mod v1;
mod ws;

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
        Self {
            config,
            db,
            tx,
            playback_state: Arc::new(RwLock::new(PlaybackState::default())),
            sync_tx,
            auth_sessions,
            auth_enabled,
            admin_user_id,
        }
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

    tokio::spawn(async move {
        for peer in &peers {
            let url = format!("ws://{}/api/sync", peer);
            info!("connecting to sync peer: {}", url);
            match tokio_tungstenite::connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    info!("connected to sync peer: {}", peer);
                    let (mut sender, mut receiver) = ws_stream.split();
                    let mut local_sync_rx = sync_tx.subscribe();

                    // Send identify
                    let identify = michi_sync::SyncMessage::Identify {
                        name: sync_name.clone(),
                        version: "0.1.0".into(),
                    };
                    if let Ok(json) = identify.serialize() {
                        let _ = sender.send(Message::Text(json)).await;
                    }

                    // Relay local state changes to peer
                    let send_task = tokio::spawn(async move {
                        while let Ok(msg) = local_sync_rx.recv().await {
                            if let Ok(json) = msg.serialize() {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    });

                    // Receive state from peer
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
                                        };
                                        {
                                            let mut current = recv_playback.write().await;
                                            *current = new_state;
                                        }
                                        let _ = recv_tx.send(format!(
                                            r#"{{"type":"sync_state","track_id":{},"position_ms":{},"playing":{},"volume":{}}}"#,
                                            track_id.map(|id| format!("\"{}\"", id)).unwrap_or_else(|| "null".into()),
                                            position_ms,
                                            playing,
                                            volume,
                                        ));
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
                    warn!("failed to connect to sync peer {}: {}", peer, e);
                }
            }
        }
    });
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
        .route("/api/v1/status", get(v1::v1_status_handler))
        .route("/api/v1/tracks", get(v1::v1_tracks_handler))
        .route("/api/v1/tracks/:id", get(v1::v1_track_handler))
        .route("/api/v1/search", get(v1::v1_search_handler))
        .route("/api/v1/stream/:id", get(v1::v1_stream_handler))
        .route("/api/v1/library/stats", get(v1::v1_stats_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state.clone());

    Router::new()
        .route("/", get(root::root_handler))
        .route("/manifest.json", get(pwa::manifest_json))
        .route("/sw.js", get(pwa::sw_js))
        .route("/api/shared/:code", get(library::shared_playlist_handler))
        .route("/api/v1/server/info", get(v1::server_info_handler))
        .merge(auth::auth_router())
        .merge(
            utoipa_swagger_ui::SwaggerUi::new("/api/docs")
                .url("/api-docs/openapi.json", ApiDoc::openapi()),
        )
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
