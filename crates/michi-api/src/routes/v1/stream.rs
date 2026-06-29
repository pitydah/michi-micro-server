use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Json,
    body::Body,
};
use tokio::io::AsyncReadExt;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub format: Option<String>,
}

pub async fn stream_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "database_error",
                "message": e.to_string()
            })))
        })?
        .ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "not_found",
                "message": format!("track not found: {}", id)
            })))
        })?;

    let music_paths = &state.config.music_paths;

    if let Some(ref format_str) = query.format {
        if !michi_streaming::check_ffmpeg() {
            return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "ffmpeg_unavailable",
                "message": "ffmpeg is not available on this system"
            }))));
        }

        let tf = format_str.parse::<michi_streaming::TranscodeFormat>().map_err(|_| {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid_format",
                "message": format!("invalid format: '{}'. Supported: mp3, ogg, hls", format_str)
            })))
        })?;

        let file_path = std::path::Path::new(&track.file_path);
        let canonical = michi_streaming::validate_track_path(music_paths, file_path)
            .map_err(|e| {
                (StatusCode::NOT_FOUND, Json(serde_json::json!({
                    "error": "file_not_found",
                    "message": e.to_string()
                })))
            })?;

        let stream = michi_streaming::transcode_stream(&canonical, &tf).await
            .map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                    "error": "transcoding_failed",
                    "message": e.to_string()
                })))
            })?;

        let mime = tf.mime_type();
        return Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .body(Body::from_stream(stream))
            .map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                    "error": "response_error",
                    "message": e.to_string()
                })))
            });
    }

    let (_path, file) = michi_streaming::open_track_file_async(music_paths, &track)
        .await
        .map_err(|e| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "file_not_found",
                "message": e.to_string()
            })))
        })?;

    let metadata = file.metadata().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "io_error",
            "message": e.to_string()
        })))
    })?;

    let file_size = metadata.len();
    let mime = track.format.mime_type();

    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header.to_str().map_err(|_| {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid_range",
                "message": "invalid range header encoding"
            })))
        })?;

        match michi_streaming::parse_range(range_str, file_size) {
            Ok(range) => {
                use tokio::io::AsyncSeekExt;
                let mut file = file;
                file.seek(std::io::SeekFrom::Start(range.start)).await.map_err(|e| {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "io_error",
                        "message": e.to_string()
                    })))
                })?;

                let taken = file.take(range.content_length());
                let stream = tokio_util::io::ReaderStream::new(taken);
                let content_range = range.content_range_header();

                Ok(Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, mime)
                    .header(header::CONTENT_RANGE, content_range)
                    .header(header::CONTENT_LENGTH, range.content_length().to_string())
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::from_stream(stream))
                    .map_err(|e| {
                        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                            "error": "response_error",
                            "message": e.to_string()
                        })))
                    })?)
            }
            Err(_) => Err((StatusCode::RANGE_NOT_SATISFIABLE, Json(serde_json::json!({
                "error": "range_not_satisfiable",
                "message": "range not satisfiable"
            })))),
        }
    } else {
        let stream = tokio_util::io::ReaderStream::new(file);
        Ok(Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .header(header::CONTENT_LENGTH, file_size.to_string())
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::from_stream(stream))
            .map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                    "error": "response_error",
                    "message": e.to_string()
                })))
            })?)
    }
}

pub async fn download_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "database_error",
                "message": e.to_string()
            })))
        })?
        .ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "not_found",
                "message": format!("track not found: {}", id)
            })))
        })?;

    let music_paths = &state.config.music_paths;
    let (_path, file) = michi_streaming::open_track_file_async(music_paths, &track)
        .await
        .map_err(|e| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "file_not_found",
                "message": e.to_string()
            })))
        })?;

    let metadata = file.metadata().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "io_error",
            "message": e.to_string()
        })))
    })?;

    let mime = track.format.mime_type();
    let filename = format!("{}.{}", track.id, track.format.as_str());
    let stream = tokio_util::io::ReaderStream::new(file);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", filename))
        .header(header::CONTENT_LENGTH, metadata.len().to_string())
        .body(Body::from_stream(stream))
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "response_error",
                "message": e.to_string()
            })))
        })?)
}
