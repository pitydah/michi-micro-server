use axum::{
    extract::{Path, State},
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

pub async fn starred_tracks_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::get_starred_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let safe_tracks: Vec<serde_json::Value> = tracks
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "duration_ms": t.duration_ms,
                "format": t.format.as_str(),
                "starred": t.starred,
                "rating": t.rating,
                "starred_at": t.starred_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "tracks": safe_tracks })))
}

#[derive(Debug, Deserialize)]
pub struct StarBody {
    pub starred: bool,
}

pub async fn star_track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StarBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let exists = michi_db::get_track(&state.db, &id)
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

    let _ = exists;
    michi_db::star_track(&state.db, &id, body.starred)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "starred": body.starred,
    })))
}

#[derive(Debug, Deserialize)]
pub struct RatingBody {
    pub rating: u8,
}

pub async fn rate_track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<RatingBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let exists = michi_db::get_track(&state.db, &id)
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

    let _ = exists;
    let rating = body.rating.min(5);
    michi_db::rate_track(&state.db, &id, rating)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "rating": rating,
    })))
}
