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
                        format!("{d}; filename=\"{filename}\""),
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
            format!("{d}; filename=\"{filename}\""),
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
    if state.disabled_modules.read().await.contains("stream") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "Streaming module is disabled",
        ));
    }

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
                &format!("track not found: {id}"),
            )
        })?;

    let music_paths = &state.config.music_paths;

    let decision = michi_streaming::resolve_stream_decision(
        &track.format,
        track.sample_rate,
        track.bit_depth.map(|d| d as u32),
        query.format.as_deref(),
        state.config.stream_profile,
        state.config.format_policy,
        state.config.resource_profile,
        state.config.max_remote_bitrate,
    )
    .map_err(|e| {
        if e.starts_with("TRANSCODING_FORBIDDEN_BY_POLICY") {
            v1_error(
                StatusCode::BAD_REQUEST,
                "TRANSCODING_FORBIDDEN_BY_POLICY",
                &e,
            )
        } else if e.starts_with("TRANSCODING_DISABLED") {
            v1_error(StatusCode::BAD_REQUEST, "TRANSCODING_DISABLED", &e)
        } else if e.starts_with("INVALID_STREAM_PROFILE") {
            v1_error(StatusCode::BAD_REQUEST, "INVALID_STREAM_PROFILE", &e)
        } else if e.starts_with("UNSUPPORTED_TRANSCODE_PLAN") {
            v1_error(StatusCode::BAD_REQUEST, "UNSUPPORTED_TRANSCODE_PLAN", &e)
        } else {
            v1_error(StatusCode::BAD_REQUEST, "STREAM_RESOLUTION_ERROR", &e)
        }
    })?;

    match decision.mode {
        michi_streaming::StreamMode::Direct => {
            stream_file(music_paths, &track, &headers, None).await
        }
        michi_streaming::StreamMode::Transcode(plan) => {
            let permit = state
                .transcode_semaphore
                .clone()
                .try_acquire_owned()
                .map_err(|_| {
                    v1_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "MAX_TRANSCODES_REACHED",
                        "maximum concurrent transcodes reached; please retry later",
                    )
                })?;

            if !michi_streaming::check_ffmpeg() {
                return Err(v1_error(
                    StatusCode::BAD_REQUEST,
                    "FFMPEG_UNAVAILABLE",
                    "ffmpeg is not available on this system",
                ));
            }

            let file_path = std::path::Path::new(&track.file_path);
            let canonical = michi_streaming::validate_track_path(music_paths, file_path)
                .map_err(|e| v1_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", &e.to_string()))?;
            let stream = michi_streaming::transcode_stream_with_plan(&canonical, &plan)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "TRANSCODING_FAILED",
                        &e.to_string(),
                    )
                })?;
            let mime = decision.output_mime_type;

            use futures_util::StreamExt;
            let owned_permit = permit;
            let monitored_stream = stream.map(move |chunk| {
                let _keep_permit = &owned_permit;
                chunk
            });

            Ok(Response::builder()
                .header(header::CONTENT_TYPE, mime)
                .body(Body::from_stream(monitored_stream))
                .unwrap())
        }
    }
}

pub async fn download_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("stream") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "Streaming module is disabled",
        ));
    }

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
                &format!("track not found: {id}"),
            )
        })?;

    let music_paths = &state.config.music_paths;
    stream_file(music_paths, &track, &headers, Some("attachment")).await
}
