use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct TracksQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn tracks_handler(
    State(state): State<AppState>,
    Query(query): Query<TracksQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let tracks = if let Some(limit) = query.limit {
        let limit = limit.clamp(1, 500);
        let offset = query.offset.unwrap_or(0).max(0);
        michi_db::list_tracks_paged(&state.db, limit, offset).await
    } else {
        michi_db::list_tracks(&state.db).await
    }
    .map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "database_error",
            "message": e.to_string()
        })))
    })?;

    // Strip file_path from response for security
    let safe_tracks: Vec<serde_json::Value> = tracks
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "album_artist": t.album_artist,
                "duration_ms": t.duration_ms,
                "format": t.format.as_str(),
                "sample_rate": t.sample_rate,
                "bit_depth": t.bit_depth,
                "channels": t.channels,
                "artwork_id": t.artwork_id,
                "genre": t.genre,
                "year": t.year,
                "track_number": t.track_number,
                "disc_number": t.disc_number,
                "created_at": t.created_at,
                "updated_at": t.updated_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "tracks": safe_tracks })))
}

pub async fn track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "database_error",
                "message": e.to_string()
            })))
        })?
        .ok_or_else(|| {
            (axum::http::StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "not_found",
                "message": format!("track not found: {}", id)
            })))
        })?;

    // Safe response without file_path
    Ok(Json(serde_json::json!({
        "id": track.id,
        "title": track.title,
        "artist": track.artist,
        "album": track.album,
        "album_artist": track.album_artist,
        "duration_ms": track.duration_ms,
        "format": track.format.as_str(),
        "sample_rate": track.sample_rate,
        "bit_depth": track.bit_depth,
        "channels": track.channels,
        "artwork_id": track.artwork_id,
        "genre": track.genre,
        "year": track.year,
        "track_number": track.track_number,
        "disc_number": track.disc_number,
        "created_at": track.created_at,
        "updated_at": track.updated_at,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

pub async fn search_handler(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    if query.q.trim().is_empty() {
        return Ok(Json(serde_json::json!({ "tracks": [] })));
    }

    let tracks = michi_db::search_tracks(&state.db, query.q.trim())
        .await
        .map_err(|e| {
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "database_error",
                "message": e.to_string()
            })))
        })?;

    let safe_tracks: Vec<serde_json::Value> = tracks
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "album_artist": t.album_artist,
                "duration_ms": t.duration_ms,
                "format": t.format.as_str(),
                "artwork_id": t.artwork_id,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "tracks": safe_tracks })))
}
