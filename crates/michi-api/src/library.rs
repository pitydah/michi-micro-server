use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use tracing::info;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub status: String,
    pub scanned: usize,
    pub saved: usize,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

#[derive(Debug, Serialize)]
pub struct PurgeResponse {
    pub deleted: usize,
}

pub async fn scan_handler(
    State(state): State<AppState>,
) -> Result<Json<ScanResponse>, (StatusCode, Json<ErrorResponse>)> {
    let music_path = &state.config.music_path;

    if !music_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("music path not found: {}", music_path.display()),
            }),
        ));
    }

    if !music_path.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("music path is not a directory: {}", music_path.display()),
            }),
        ));
    }

    info!("scanning music library at {}", music_path.display());

    let tracks = michi_scanner::scan_directory(music_path).await;
    let scanned = tracks.len();

    let saved = michi_db::upsert_tracks(&state.db, &tracks)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    info!("scan complete: {} tracks scanned, {} saved", scanned, saved);

    Ok(Json(ScanResponse {
        status: "ok".to_string(),
        scanned,
        saved,
    }))
}

pub async fn tracks_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<ErrorResponse>)> {
    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    Ok(Json(tracks))
}

pub async fn stats_handler(
    State(state): State<AppState>,
) -> Result<Json<michi_core::LibraryStats>, (StatusCode, Json<ErrorResponse>)> {
    let stats = michi_db::library_stats(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    Ok(Json(stats))
}

pub async fn track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<michi_core::Track>, (StatusCode, Json<ErrorResponse>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("track not found: {}", id),
                }),
            )
        })?;

    Ok(Json(track))
}

pub async fn delete_track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = michi_db::delete_track(&state.db, &id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    if deleted {
        Ok(Json(DeleteResponse { deleted: true }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("track not found: {}", id),
            }),
        ))
    }
}

pub async fn update_track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(update): Json<michi_core::TrackUpdate>,
) -> Result<Json<michi_core::Track>, (StatusCode, Json<ErrorResponse>)> {
    let updated = michi_db::update_track(&state.db, &id, &update)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    if !updated {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("track not found: {}", id),
            }),
        ));
    }

    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: "track lost after update".to_string(),
                }),
            )
        })?;

    Ok(Json(track))
}

pub async fn delete_all_tracks_handler(
    State(state): State<AppState>,
) -> Result<Json<PurgeResponse>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = michi_db::delete_all_tracks(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    Ok(Json(PurgeResponse {
        deleted: deleted as usize,
    }))
}
