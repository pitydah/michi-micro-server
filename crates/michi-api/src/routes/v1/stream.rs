use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Json,
};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub format: Option<String>,
}

fn v1_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code, "message": message, "details": {} }
        })),
    )
}

async fn stream_file(
    music_paths: &[std::path::PathBuf],
    track: &michi_core::Track,
    headers: &HeaderMap,
    disposition: Option<&str>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let (_path, file) = michi_streaming::open_track_file_async(music_paths, track)
        .await
        .map_err(|e| v1_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", &e.to_string()))?;

    let metadata = file.metadata().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &e.to_string(),
        )
    })?;

    let file_size = metadata.len();
    let mime = track.format.mime_type();
    let filename = format!("{}.{}", track.id, track.format.as_str());

    let mut builder = Response::builder();

    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header.to_str().map_err(|_| {
            v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_RANGE",
                "invalid range header encoding",
            )
        })?;

        match michi_streaming::parse_range(range_str, file_size) {
            Ok(range) => {
                let mut file = file;
                file.seek(std::io::SeekFrom::Start(range.start))
                    .await
                    .map_err(|e| {
                        v1_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "IO_ERROR",
                            &e.to_string(),
                        )
                    })?;
                let taken = file.take(range.content_length());
                let stream = tokio_util::io::ReaderStream::new(taken);
                let content_range = range.content_range_header();
                builder = builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, content_range)
                    .header(header::CONTENT_LENGTH, range.content_length().to_string());
                if let Some(d) = disposition {
                    builder = builder.header(
                        header::CONTENT_DISPOSITION,
                        format!("{}; filename=\"{}\"", d, filename),
                    );
                }
                return Ok(builder
                    .header(header::CONTENT_TYPE, mime)
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::from_stream(stream))
                    .unwrap());
            }
            Err(_) => {
                return Err(v1_error(
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "RANGE_NOT_SATISFIABLE",
                    "range not satisfiable",
                ));
            }
        }
    }

    let stream = tokio_util::io::ReaderStream::new(file);
    builder = builder
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::ACCEPT_RANGES, "bytes");
    if let Some(d) = disposition {
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            format!("{}; filename=\"{}\"", d, filename),
        );
    }
    Ok(builder.body(Body::from_stream(stream)).unwrap())
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
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "TRACK_NOT_FOUND",
                &format!("track not found: {}", id),
            )
        })?;

    let music_paths = &state.config.music_paths;

    if let Some(ref format_str) = query.format {
        if !michi_streaming::check_ffmpeg() {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "FFMPEG_UNAVAILABLE",
                "ffmpeg is not available on this system",
            ));
        }
        let tf = format_str
            .parse::<michi_streaming::TranscodeFormat>()
            .map_err(|_| {
                v1_error(
                    StatusCode::BAD_REQUEST,
                    "INVALID_FORMAT",
                    &format!("invalid format: '{format_str}'. Supported: mp3, ogg, hls"),
                )
            })?;
        let file_path = std::path::Path::new(&track.file_path);
        let canonical = michi_streaming::validate_track_path(music_paths, file_path)
            .map_err(|e| v1_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", &e.to_string()))?;
        let stream = michi_streaming::transcode_stream(&canonical, &tf)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "TRANSCODING_FAILED",
                    &e.to_string(),
                )
            })?;
        let mime = tf.mime_type();
        return Ok(Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .body(Body::from_stream(stream))
            .unwrap());
    }

    stream_file(music_paths, &track, &headers, None).await
}

pub async fn download_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "TRACK_NOT_FOUND",
                &format!("track not found: {}", id),
            )
        })?;

    let music_paths = &state.config.music_paths;
    stream_file(music_paths, &track, &headers, Some("attachment")).await
}
