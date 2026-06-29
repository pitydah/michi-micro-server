use axum::{extract::State, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

pub async fn sync_manifest_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::get_all_tracks_manifest(&state.db).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "database_error",
            "message": e.to_string()
        })))
    })?;

    let manifest: Vec<serde_json::Value> = tracks
        .into_iter()
        .map(|(track_id, file_path, title, artist, album, duration_ms, artwork_id)| {
            serde_json::json!({
                "track_id": track_id,
                "file_path": file_path,
                "title": title,
                "artist": artist,
                "album": album,
                "duration_ms": duration_ms,
                "artwork_id": if artwork_id.is_empty() { None } else { Some(artwork_id) },
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "tracks": manifest,
        "total": manifest.len(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct DeltaRequest {
    pub known_ids: Vec<Uuid>,
}

pub async fn sync_manifest_delta_handler(
    State(state): State<AppState>,
    Json(body): Json<DeltaRequest>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let all = michi_db::get_all_tracks_manifest(&state.db).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "database_error",
            "message": e.to_string()
        })))
    })?;

    let known: std::collections::HashSet<Uuid> = body.known_ids.into_iter().collect();
    let mut added = Vec::new();
    let mut removed: Vec<Uuid> = Vec::new();

    for (track_id, file_path, title, artist, album, duration_ms, artwork_id) in &all {
        if !known.contains(track_id) {
            added.push(serde_json::json!({
                "track_id": track_id,
                "file_path": file_path,
                "title": title,
                "artist": artist,
                "album": album,
                "duration_ms": duration_ms,
                "artwork_id": if artwork_id.is_empty() { None } else { Some(artwork_id) },
            }));
        }
    }

    // Check for removed tracks: known IDs not in current manifest
    let current_ids: std::collections::HashSet<Uuid> = all.iter().map(|(id, ..)| *id).collect();
    for known_id in known {
        if !current_ids.contains(&known_id) {
            removed.push(known_id);
        }
    }

    Ok(Json(serde_json::json!({
        "added": added,
        "removed": removed,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SyncStateBody {
    pub track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: f64,
}

pub async fn sync_state_handler(
    State(state): State<AppState>,
    Json(body): Json<SyncStateBody>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let new_state = michi_sync::PlaybackState {
        track_id: body.track_id,
        position_ms: body.position_ms,
        playing: body.playing,
        volume: body.volume,
        updated_at: chrono::Utc::now(),
    };

    {
        let mut current = state.playback_state.write().await;
        *current = new_state.clone();
    }

    let _ = state.sync_tx.send(new_state.into());
    let _ = state.tx.send(serde_json::json!({
        "type": "sync_state",
        "track_id": body.track_id,
        "position_ms": body.position_ms,
        "playing": body.playing,
    }).to_string());

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
