use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Json,
    body::Body,
};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub format: Option<String>,
}

fn v1_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({
        "error": { "code": code, "message": message }
    })))
}

fn v1_internal_error(code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    v1_error(StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

pub async fn stream_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| v1_internal_error("DATABASE_ERROR", &e.to_string()))?
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "TRACK_NOT_FOUND", &format!("track not found: {}", id)))?;

    let music_paths = &state.config.music_paths;

    if let Some(ref format_str) = query.format {
        if !michi_streaming::check_ffmpeg() {
            return Err(v1_error(StatusCode::BAD_REQUEST, "FFMPEG_UNAVAILABLE", "ffmpeg is not available on this system"));
        }

        let tf = format_str.parse::<michi_streaming::TranscodeFormat>().map_err(|_| {
            v1_error(StatusCode::BAD_REQUEST, "INVALID_FORMAT", &format!("invalid format: '{format_str}'. Supported: mp3, ogg, hls"))
        })?;

        let file_path = std::path::Path::new(&track.file_path);
        let canonical = michi_streaming::validate_track_path(music_paths, file_path)
            .map_err(|e| v1_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", &e.to_string()))?;

        let stream = michi_streaming::transcode_stream(&canonical, &tf).await
            .map_err(|e| v1_internal_error("TRANSCODING_FAILED", &e.to_string()))?;

        let mime = tf.mime_type();
        return Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .body(Body::from_stream(stream))
            .map_err(|e| v1_internal_error("RESPONSE_ERROR", &e.to_string()));
    }

    let (_path, file) = michi_streaming::open_track_file_async(music_paths, &track)
        .await
        .map_err(|e| v1_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", &e.to_string()))?;

    let metadata = file.metadata().await.map_err(|e| {
        v1_internal_error("IO_ERROR", &e.to_string())
    })?;

    let file_size = metadata.len();
    let mime = track.format.mime_type();

    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header.to_str().map_err(|_| {
            v1_error(StatusCode::BAD_REQUEST, "INVALID_RANGE", "invalid range header encoding")
        })?;

        match michi_streaming::parse_range(range_str, file_size) {
            Ok(range) => {
                let mut file = file;
                file.seek(std::io::SeekFrom::Start(range.start)).await.map_err(|e| {
                    v1_internal_error("IO_ERROR", &e.to_string())
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
                    .map_err(|e| v1_internal_error("RESPONSE_ERROR", &e.to_string()))?)
            }
            Err(_) => Err(v1_error(StatusCode::RANGE_NOT_SATISFIABLE, "RANGE_NOT_SATISFIABLE", "range not satisfiable")),
        }
    } else {
        let stream = tokio_util::io::ReaderStream::new(file);
        Ok(Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .header(header::CONTENT_LENGTH, file_size.to_string())
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::from_stream(stream))
            .map_err(|e| v1_internal_error("RESPONSE_ERROR", &e.to_string()))?)
    }
}

pub async fn download_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| v1_internal_error("DATABASE_ERROR", &e.to_string()))?
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "TRACK_NOT_FOUND", &format!("track not found: {}", id)))?;

    let music_paths = &state.config.music_paths;
    let (_path, file) = michi_streaming::open_track_file_async(music_paths, &track)
        .await
        .map_err(|e| v1_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", &e.to_string()))?;

    let metadata = file.metadata().await.map_err(|e| {
        v1_internal_error("IO_ERROR", &e.to_string())
    })?;

    let mime = track.format.mime_type();
    let filename = format!("{}.{}", track.id, track.format.as_str());
    let stream = tokio_util::io::ReaderStream::new(file);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", filename))
        .header(header::CONTENT_LENGTH, metadata.len().to_string())
        .body(Body::from_stream(stream))
        .map_err(|e| v1_internal_error("RESPONSE_ERROR", &e.to_string()))?)
}
