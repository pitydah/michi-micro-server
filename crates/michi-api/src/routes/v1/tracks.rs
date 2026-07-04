use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

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

#[derive(Debug, Deserialize)]
pub struct TracksQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub fn track_to_safe_json(track: michi_core::Track) -> serde_json::Value {
    let track_id = track.id;
    serde_json::json!({
        "id": track_id,
        "title": track.title,
        "artist": track.artist,
        "album": track.album,
        "album_artist": track.album_artist,
        "duration_ms": track.duration_ms,
        "format": track.format.as_str(),
        "sample_rate": track.sample_rate,
        "bit_depth": track.bit_depth,
        "channels": track.channels,
        "cover_id": track.artwork_id,
        "artwork_id": track.artwork_id,
        "genre": track.genre,
        "year": track.year,
        "track_number": track.track_number,
        "disc_number": track.disc_number,
        "created_at": track.created_at,
        "updated_at": track.updated_at,
        "stream_url": format!("/api/v1/stream/{}", track_id),
        "download_url": format!("/api/v1/download/{}", track_id),
        "starred": track.starred,
        "rating": track.rating,
        "starred_at": track.starred_at,
    })
}

pub async fn tracks_handler(
    State(state): State<AppState>,
    Query(query): Query<TracksQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks = if let Some(limit) = query.limit {
        let limit = limit.clamp(1, 500);
        let offset = query.offset.unwrap_or(0).max(0);
        michi_db::list_tracks_paged(&state.db, limit, offset).await
    } else {
        michi_db::list_tracks(&state.db).await
    }
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let safe_tracks: Vec<serde_json::Value> = tracks.into_iter().map(track_to_safe_json).collect();

    Ok(Json(serde_json::json!({ "tracks": safe_tracks })))
}

pub async fn track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
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

    Ok(Json(track_to_safe_json(track)))
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

pub async fn search_handler(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if query.q.trim().is_empty() {
        return Ok(Json(serde_json::json!({ "tracks": [] })));
    }

    let tracks = michi_db::search_tracks(&state.db, query.q.trim())
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let safe_tracks: Vec<serde_json::Value> = tracks.into_iter().map(track_to_safe_json).collect();

    Ok(Json(serde_json::json!({ "tracks": safe_tracks })))
}
