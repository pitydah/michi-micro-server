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
                StatusCode::NOT_FOUND => "TRACK_NOT_FOUND",
                StatusCode::FORBIDDEN => "FORBIDDEN",
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
