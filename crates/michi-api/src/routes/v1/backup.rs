use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

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

#[derive(Serialize)]
struct BackupPayload {
    version: i32,
    exported_at: String,
    tracks: Vec<michi_core::Track>,
    playlists: Vec<michi_core::Playlist>,
    starred_tracks: Vec<michi_core::Track>,
    play_history: Vec<PlayHistoryEntry>,
    server_id: String,
    server_name: String,
}

#[derive(Serialize)]
struct PlayHistoryEntry {
    track_id: String,
    played_at: String,
    timestamp: String,
}

pub async fn backup_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let playlists = michi_db::list_playlists(&state.db, None).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let starred_tracks = michi_db::get_starred_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let play_history_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT track_id, played_at FROM play_history ORDER BY played_at DESC LIMIT 10000")
            .fetch_all(&state.db)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;

    let play_history: Vec<PlayHistoryEntry> = play_history_rows
        .into_iter()
        .map(|(track_id, played_at)| {
            let timestamp = played_at.clone();
            PlayHistoryEntry {
                track_id,
                played_at,
                timestamp,
            }
        })
        .collect();

    let server_id = state.server_id().to_string();
    let server_name = "Michi Micro Server".to_string();

    let backup = BackupPayload {
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        tracks,
        playlists,
        starred_tracks,
        play_history,
        server_id,
        server_name,
    };

    Ok(Json(serde_json::json!(backup)))
}
