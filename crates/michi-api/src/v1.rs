use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{library, status, stream, AppState};

#[derive(Debug, Serialize)]
pub struct V1Error {
    pub error: V1ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct V1ErrorBody {
    pub code: String,
    pub message: String,
}

impl V1Error {
    fn new(code: &str, message: String) -> Self {
        V1Error {
            error: V1ErrorBody {
                code: code.to_string(),
                message,
            },
        }
    }
}

fn v1_map_err(status: StatusCode, msg: &str, code: &str) -> (StatusCode, Json<V1Error>) {
    (status, Json(V1Error::new(code, msg.to_string())))
}

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub server_id: Uuid,
    pub version: String,
    pub api_version: String,
    pub features: ServerFeatures,
}

#[derive(Debug, Serialize)]
pub struct ServerFeatures {
    pub library: bool,
    pub search: bool,
    pub streaming: bool,
    pub web_ui: bool,
    pub playlists: bool,
    pub artwork: bool,
    pub sync: bool,
    pub transcoding: bool,
    pub websocket: bool,
}

pub async fn server_info_handler(State(state): State<AppState>) -> Json<ServerInfo> {
    Json(ServerInfo {
        name: "Michi Micro Server".to_string(),
        server_id: state.server_id(),
        version: state.config.version.to_string(),
        api_version: "v1".to_string(),
        features: ServerFeatures {
            library: true,
            search: true,
            streaming: true,
            web_ui: true,
            playlists: true,
            artwork: true,
            sync: false,
            transcoding: false,
            websocket: true,
        },
    })
}

pub async fn v1_status_handler(State(state): State<AppState>) -> Json<status::StatusResponse> {
    status::status_handler(State(state)).await
}

pub async fn v1_tracks_handler(
    State(state): State<AppState>,
    Query(query): Query<library::TracksQuery>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<V1Error>)> {
    library::tracks_handler(State(state), Query(query))
        .await
        .map_err(|(status, err)| v1_map_err(status, &err.message, "INTERNAL_ERROR"))
}

pub async fn v1_track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<michi_core::Track>, (StatusCode, Json<V1Error>)> {
    library::track_handler(State(state), Path(id))
        .await
        .map_err(|(status, err)| {
            let code = match status {
                StatusCode::NOT_FOUND => "TRACK_NOT_FOUND",
                StatusCode::BAD_REQUEST => "INVALID_ID",
                _ => "INTERNAL_ERROR",
            };
            v1_map_err(status, &err.message, code)
        })
}

pub async fn v1_search_handler(
    State(state): State<AppState>,
    Query(query): Query<library::SearchParams>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<V1Error>)> {
    library::search_handler(State(state), Query(query))
        .await
        .map_err(|(status, err)| v1_map_err(status, &err.message, "INTERNAL_ERROR"))
}

pub async fn v1_stream_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<stream::StreamQuery>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, (StatusCode, Json<V1Error>)> {
    stream::stream_handler(State(state), Path(id), Query(query), headers)
        .await
        .map_err(|(status, err)| {
            let code = match status {
                StatusCode::NOT_FOUND => "FILE_NOT_FOUND",
                StatusCode::FORBIDDEN => "FORBIDDEN",
                StatusCode::RANGE_NOT_SATISFIABLE => "RANGE_NOT_SATISFIABLE",
                StatusCode::BAD_REQUEST => "BAD_REQUEST",
                _ => "STREAM_ERROR",
            };
            v1_map_err(status, &err.0.message, code)
        })
}

pub async fn v1_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<michi_core::LibraryStats>, (StatusCode, Json<V1Error>)> {
    library::stats_handler(State(state))
        .await
        .map_err(|(status, err)| v1_map_err(status, &err.message, "INTERNAL_ERROR"))
}

pub async fn v1_playlists_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<michi_core::Playlist>>, (StatusCode, Json<V1Error>)> {
    library::playlists_handler(State(state), headers)
        .await
        .map_err(|(status, err)| v1_map_err(status, &err.message, "INTERNAL_ERROR"))
}

pub async fn v1_create_playlist_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<michi_core::PlaylistCreate>,
) -> Result<Json<michi_core::Playlist>, (StatusCode, Json<V1Error>)> {
    library::create_playlist_handler(State(state), headers, Json(input))
        .await
        .map_err(|(status, err)| v1_map_err(status, &err.message, "INTERNAL_ERROR"))
}

pub async fn v1_get_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<michi_core::Playlist>, (StatusCode, Json<V1Error>)> {
    library::get_playlist_handler(State(state), Path(id))
        .await
        .map_err(|(status, err)| {
            let code = if status == StatusCode::NOT_FOUND {
                "NOT_FOUND"
            } else {
                "INTERNAL_ERROR"
            };
            v1_map_err(status, &err.message, code)
        })
}

pub async fn v1_delete_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<library::DeleteResponse>, (StatusCode, Json<V1Error>)> {
    library::delete_playlist_handler(State(state), Path(id))
        .await
        .map_err(|(status, err)| v1_map_err(status, &err.message, "INTERNAL_ERROR"))
}

pub async fn v1_get_playlist_tracks_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<V1Error>)> {
    library::get_playlist_tracks_handler(State(state), Path(id))
        .await
        .map_err(|(status, err)| v1_map_err(status, &err.message, "INTERNAL_ERROR"))
}

pub async fn v1_artwork_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, (StatusCode, Json<V1Error>)> {
    match library::artwork_handler(State(state), Path(id)).await {
        Ok(resp) => Ok(resp),
        Err((status, err)) => {
            let code = if status == StatusCode::NOT_FOUND {
                "NOT_FOUND"
            } else {
                "INTERNAL_ERROR"
            };
            Err(v1_map_err(status, &err.message, code))
        }
    }
}

pub async fn v1_hls_segment_handler(
    State(state): State<AppState>,
    Path((id, segment)): Path<(String, String)>,
) -> Result<axum::response::Response, (StatusCode, Json<V1Error>)> {
    crate::stream::hls_segment_handler(State(state), Path((id, segment)))
        .await
        .map_err(|(status, err)| {
            let code = match status {
                StatusCode::NOT_FOUND => "NOT_FOUND",
                _ => "INTERNAL_ERROR",
            };
            v1_map_err(status, &err.message, code)
        })
}

pub async fn v1_ws_handler(
    ws: axum::extract::ws::WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    crate::ws::ws_handler(ws, State(state)).await
}
