use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{library::ErrorResponse, AppState};
use michi_streaming::{
    open_track_file_async, parse_range, read_range_from_file_async, StreamError,
};

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

async fn track_from_db(
    state: &AppState,
    id: &uuid::Uuid,
) -> Result<michi_core::Track, (StatusCode, Json<ErrorResponse>)> {
    michi_db::get_track(&state.db, id)
        .await
        .map_err(|e| {
            err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("db error: {}", e),
            )
        })?
        .ok_or_else(|| err_response(StatusCode::NOT_FOUND, &format!("track not found: {}", id)))
}

pub async fn stream_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    let track = track_from_db(&state, &id).await?;
    let music_path = &state.config.music_path;

    let (_canonical, mut file) = open_track_file_async(music_path, &track)
        .await
        .map_err(|e| match e {
            StreamError::FileNotFound(msg) => err_response(StatusCode::NOT_FOUND, &msg),
            StreamError::UnsafePath(msg) => {
                tracing::warn!("unsafe path access attempt: {}", msg);
                err_response(StatusCode::FORBIDDEN, "access denied")
            }
            _ => {
                tracing::error!("error opening track file: {}", e);
                err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
            }
        })?;

    let metadata = file.metadata().await.map_err(|e| {
        tracing::error!("failed to get metadata: {}", e);
        err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
    })?;

    let file_size = metadata.len();
    let mime = mime_from_ext(std::path::Path::new(&track.file_path));

    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header
            .to_str()
            .map_err(|_| err_response(StatusCode::BAD_REQUEST, "invalid range header encoding"))?;

        match parse_range(range_str, file_size) {
            Ok(range) => {
                let data = read_range_from_file_async(&mut file, &range)
                    .await
                    .map_err(|e| {
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
        let stream = ReaderStream::new(file);
        let body = Body::from_stream(stream);

        let resp = Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .header(header::CONTENT_LENGTH, file_size.to_string())
            .header(header::ACCEPT_RANGES, "bytes")
            .body(body)
            .map_err(|e| {
                tracing::error!("failed to build response: {}", e);
                err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
            })?;

        Ok(resp)
    }
}
