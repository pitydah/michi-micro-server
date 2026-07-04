use std::sync::Arc;
use std::time::Instant;

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
        michi_link::spawn_token_cleanup(token_store.clone());
        let playback_state = Arc::new(RwLock::new(PlaybackState::default()));
        let receiver_manager = michi_receivers::ReceiverSessionManager::new();
        routes::v1::import::spawn_import_cleanup(&config, db.clone());
        routes::v1::playback::auto_restore_playback_state(db.clone(), playback_state.clone());
        // Spawn receiver heartbeat monitor
        let rm = receiver_manager.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let reg = rm.registry().await;
                let reg_read = reg.read().await;
                let now = chrono::Utc::now();
                for entry in reg_read.list() {
                    if let Some(last) = entry.last_seen {
                        if (now - last).num_seconds() > 180 {
                            tracing::warn!(
                                "receiver {} not seen for >180s, marking offline",
                                entry.receiver_id
                            );
                        }
                    }
                }
            }
        });
        Self {
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

                    let identify = michi_sync::SyncMessage::Identify {
                        name: sync_name.clone(),
                        version: "0.1.0".into(),
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
                    warn!("failed to connect to sync peer {}: {}", peer, e);
                }
            }
        }
    });
}

fn v1_link_routes() -> Router<AppState> {
    Router::new()
        // Server
        .route(
            "/api/v1/server/info",
            get(routes::v1::server::server_info_handler),
        )
        .route("/api/v1/status", get(routes::v1::server::status_handler))
        // Pairing
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
            "/api/v1/devices/revoke",
            post(routes::v1::pair::link_devices_revoke),
        )
        // Library
        .route(
            "/api/v1/library/stats",
            get(routes::v1::library::library_stats_handler),
        )
        .route(
            "/api/v1/library/scan",
            post(routes::v1::library::library_scan_handler),
        )
        // Tracks
        .route("/api/v1/tracks", get(routes::v1::tracks::tracks_handler))
        .route("/api/v1/tracks/:id", get(routes::v1::tracks::track_handler))
        .route("/api/v1/search", get(routes::v1::tracks::search_handler))
        // Stream
        .route(
            "/api/v1/stream/:id",
            get(routes::v1::stream::stream_handler),
        )
        .route(
            "/api/v1/download/:id",
            get(routes::v1::stream::download_handler),
        )
        // Artwork
        .route(
            "/api/v1/artwork/:id",
            get(routes::v1::artwork::artwork_handler),
        )
        // Playlists
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
        // Favorites / Star / Rating
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
        // Sync
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
        // Import
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
        // Diagnostics
        .route(
            "/api/v1/diagnostics",
            get(routes::v1::diagnostics::diagnostics_handler),
        )
        // Playback
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
        // Queue
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
        // Receivers
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
        // Rooms
        .route(
            "/api/v1/rooms",
            get(routes::v1::rooms::rooms_handler).post(routes::v1::rooms::create_room_handler),
        )
        .route(
            "/api/v1/rooms/:id/play",
            post(routes::v1::rooms::room_play_handler),
        )
        // Events
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
        .with_state(state.clone());

    Router::new()
        .route("/", get(root::root_handler))
        .route("/static/styles.css", get(static_files::styles_css))
        .route("/static/app.js", get(static_files::app_js))
        .route("/static/assets/michi-logo.svg", get(static_files::logo_svg))
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
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer(&state))
        .with_state(state)
}

fn cors_layer(state: &AppState) -> CorsLayer {
    if state.config.dev_mode {
        return CorsLayer::permissive();
    }
    if let Some(ref origin) = state.config.cors_origin {
        match origin.parse::<axum::http::HeaderValue>() {
            Ok(header_origin) => CorsLayer::new()
                .allow_origin(tower_http::cors::AllowOrigin::exact(header_origin))
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
            Err(_) => {
                tracing::warn!("invalid MICHI_CORS_ORIGIN value, using restrictive CORS");
                CorsLayer::new()
            }
        }
    } else {
        CorsLayer::new()
    }
}
