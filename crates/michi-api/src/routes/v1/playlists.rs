use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

pub async fn playlists_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let playlists = michi_db::list_playlists(&state.db, None).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
        })))
    })?;

    Ok(Json(serde_json::json!({ "playlists": playlists })))
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaylistBody {
    pub name: String,
    pub description: Option<String>,
}

pub async fn create_playlist_handler(
    State(state): State<AppState>,
    Json(body): Json<CreatePlaylistBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": { "code": "VALIDATION_ERROR", "message": "playlist name is required" }
        }))));
    }

    let input = michi_core::PlaylistCreate {
        name: body.name.trim().to_string(),
        description: body.description,
    };

    let playlist = michi_db::create_playlist(&state.db, &input, None).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
        })))
    })?;

    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());

    Ok(Json(serde_json::json!({ "playlist": playlist })))
}

pub async fn get_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let playlist = michi_db::get_playlist(&state.db, &id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
        })))
    })?
    .ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": { "code": "NOT_FOUND", "message": format!("playlist not found: {}", id) }
        })))
    })?;

    Ok(Json(serde_json::json!({ "playlist": playlist })))
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlaylistBody {
    pub name: Option<String>,
    pub description: Option<String>,
}

pub async fn update_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(_body): Json<UpdatePlaylistBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _playlist = michi_db::get_playlist(&state.db, &id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
        })))
    })?
    .ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": { "code": "NOT_FOUND", "message": format!("playlist not found: {}", id) }
        })))
    })?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

pub async fn delete_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_playlist(&state.db, &id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
        })))
    })?;

    if !deleted {
        return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": { "code": "NOT_FOUND", "message": format!("playlist not found: {}", id) }
        }))));
    }

    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}
