use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use crate::{library::ErrorResponse, AppState};
use michi_streaming::{open_track_file, parse_range, StreamError};

fn err_response(status: StatusCode, msg: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            status: "error".to_string(),
            message: msg.to_string(),
        }),
    )
}

fn mime_from_ext(file_path: &std::path::Path) -> &'static str {
    file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(michi_streaming::mime_type_for_ext)
        .unwrap_or("application/octet-stream")
}

pub async fn stream_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    let id_str = id.to_string();

    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("db error: {}", e),
            )
        })?
        .ok_or_else(|| {
            err_response(
                StatusCode::NOT_FOUND,
                &format!("track not found: {}", id_str),
            )
        })?;

    let music_path = &state.config.music_path;

    let (_canonical, file) = open_track_file(music_path, &track).map_err(|e| match e {
        StreamError::FileNotFound(msg) => err_response(StatusCode::NOT_FOUND, &msg),
        StreamError::UnsafePath(msg) => {
            tracing::warn!("unsafe path access attempt: {}", msg);
            err_response(StatusCode::NOT_FOUND, "file not found")
        }
        _ => {
            tracing::error!("error opening track file: {}", e);
            err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
        }
    })?;

    let file_size = file
        .metadata()
        .map_err(|e| {
            tracing::error!("failed to get metadata: {}", e);
            err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
        })?
        .len();

    let mime = mime_from_ext(std::path::Path::new(&track.file_path));

    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header
            .to_str()
            .map_err(|_| err_response(StatusCode::BAD_REQUEST, "invalid range header encoding"))?;

        match parse_range(range_str, file_size) {
            Ok(range) => {
                let data = michi_streaming::read_range_from_file(&file, &range).map_err(|e| {
                    tracing::error!("error reading range: {}", e);
                    err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                })?;

                let content_range = range.content_range_header();
                let resp = (
                    StatusCode::PARTIAL_CONTENT,
                    [
                        (header::CONTENT_TYPE, mime),
                        (header::CONTENT_RANGE, content_range.as_str()),
                        (header::CONTENT_LENGTH, &data.len().to_string()),
                        (header::ACCEPT_RANGES, "bytes"),
                    ],
                    data,
                )
                    .into_response();
                Ok(resp)
            }
            Err(StreamError::InvalidRange(msg)) => {
                let cr = format!("bytes */{}", file_size);
                let body = Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("range not satisfiable: {}", msg),
                });
                let resp = (
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    [(header::CONTENT_RANGE, cr.as_str())],
                    body,
                )
                    .into_response();
                Ok(resp)
            }
            Err(_) => Err(err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error",
            )),
        }
    } else {
        let content = std::fs::read(&_canonical).map_err(|e| {
            tracing::error!("error reading file: {}", e);
            err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
        })?;

        let resp = (
            [
                (header::CONTENT_TYPE, mime),
                (header::ACCEPT_RANGES, "bytes"),
            ],
            content,
        )
            .into_response();
        Ok(resp)
    }
}
