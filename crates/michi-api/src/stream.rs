use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Json,
};
use serde::Deserialize;
use std::io;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{library::ErrorResponse, AppState};
use michi_streaming::{
    check_ffmpeg, open_track_file_async, parse_range, transcode_stream, StreamError,
    TranscodeFormat,
};

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub format: Option<String>,
}

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

pub async fn stream_range_async(
    mut file: tokio::fs::File,
    range: &michi_streaming::ByteRange,
    mime: &str,
) -> Result<Response, StreamError> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncSeekExt;

    file.seek(std::io::SeekFrom::Start(range.start)).await?;

    let taken = file.take(range.content_length());
    let stream = ReaderStream::new(taken);
    let body = Body::from_stream(stream);

    let content_range = range.content_range_header();
    let content_length = range.content_length().to_string();

    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_RANGE, &content_range)
        .header(header::CONTENT_LENGTH, &content_length)
        .header(header::ACCEPT_RANGES, "bytes")
        .body(body)
        .map_err(|e| StreamError::Io(io::Error::other(e.to_string())))
}

#[utoipa::path(
    get,
    path = "/api/stream/{id}",
    tag = "Streaming",
    params(
        ("id" = Uuid, Path, description = "Track UUID"),
        ("format" = Option<String>, Query, description = "Transcode format (mp3, ogg)"),
    ),
    responses(
        (status = 200, description = "Audio stream"),
        (status = 206, description = "Partial content (range request)"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 404, description = "Track not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn stream_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    let track = track_from_db(&state, &id).await?;
    let music_paths = &state.config.music_paths;

    // If format query param is specified, transcode via ffmpeg
    if let Some(ref format_str) = query.format {
        if !check_ffmpeg() {
            return Err(err_response(
                StatusCode::BAD_REQUEST,
                "ffmpeg is not available on this system",
            ));
        }

        let tf = format_str.parse::<TranscodeFormat>().map_err(|_| {
            err_response(
                StatusCode::BAD_REQUEST,
                &format!("invalid format: '{format_str}'. Supported: mp3, ogg, hls"),
            )
        })?;

        let file_path = std::path::Path::new(&track.file_path);
        let canonical =
            michi_streaming::validate_track_path(music_paths, file_path).map_err(|e| match e {
                StreamError::FileNotFound(msg) => err_response(StatusCode::NOT_FOUND, &msg),
                StreamError::UnsafePath(msg) => {
                    tracing::warn!("unsafe path access attempt: {}", msg);
                    err_response(StatusCode::FORBIDDEN, "access denied")
                }
                _ => {
                    tracing::error!("error validating track path: {e}");
                    err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                }
            })?;

        if tf == TranscodeFormat::Hls {
            let cache_path = &state.config.cache_path;
            let track_id = id.to_string();
            // Generate HLS segments if not already cached
            let playlist_path =
                michi_streaming::hls_output_dir(cache_path, &track_id).join("playlist.m3u8");
            if !tokio::fs::try_exists(&playlist_path).await.unwrap_or(false) {
                michi_streaming::generate_hls_playlist(&canonical, cache_path, &track_id)
                    .await
                    .map_err(|e| {
                        tracing::error!("HLS generation failed: {e}");
                        err_response(StatusCode::INTERNAL_SERVER_ERROR, "HLS generation failed")
                    })?;
            }
            let playlist = michi_streaming::read_hls_playlist(cache_path, &track_id)
                .await
                .map_err(|e| {
                    tracing::error!("HLS playlist read failed: {e}");
                    err_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "HLS playlist read failed",
                    )
                })?;
            // Replace relative segment paths with absolute URLs
            let prefix = format!("/api/hls/{track_id}/");
            let playlist = playlist
                .lines()
                .map(|line| {
                    if line.ends_with(".ts") {
                        format!("{prefix}{line}")
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Response::builder()
                .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                .body(Body::from(playlist))
                .map_err(|e| {
                    tracing::error!("failed to build HLS response: {e}");
                    err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                });
        }

        let stream = transcode_stream(&canonical, &tf).await.map_err(|e| {
            tracing::error!("transcoding failed: {e}");
            err_response(StatusCode::INTERNAL_SERVER_ERROR, "transcoding failed")
        })?;

        let body = Body::from_stream(stream);
        let mime = tf.mime_type();

        let resp = Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .body(body)
            .map_err(|e| {
                tracing::error!("failed to build response: {e}");
                err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
            })?;

        return Ok(resp);
    }

    let (_, file) = open_track_file_async(music_paths, &track)
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
                let resp = stream_range_async(file, &range, mime).await.map_err(|e| {
                    tracing::error!("error streaming range: {}", e);
                    err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                })?;
                Ok(resp)
            }
            Err(StreamError::InvalidRange(msg)) => Err(err_response(
                StatusCode::RANGE_NOT_SATISFIABLE,
                &format!("range not satisfiable: {}", msg),
            )),
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

pub async fn hls_segment_handler(
    State(state): State<AppState>,
    Path((id, segment)): Path<(String, String)>,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    use crate::library::ErrorResponse;
    fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<ErrorResponse>) {
        (
            status,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: msg.to_string(),
            }),
        )
    }

    let segment_path = michi_streaming::hls_segment_path(&state.config.cache_path, &id, &segment);
    if !segment_path.exists() {
        return Err(err(StatusCode::NOT_FOUND, "HLS segment not found"));
    }

    let data = tokio::fs::read(&segment_path).await.map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to read segment: {e}"),
        )
    })?;

    Response::builder()
        .header(header::CONTENT_TYPE, "video/MP2T")
        .header(header::CONTENT_LENGTH, data.len().to_string())
        .body(Body::from(data))
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("response error: {e}"),
            )
        })
}
