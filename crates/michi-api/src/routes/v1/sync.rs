use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

fn v1_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({
        "error": { "code": code, "message": message, "details": {} }
    })))
}

pub async fn sync_manifest_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::get_all_tracks_manifest(&state.db).await.map_err(|e| {
        v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string())
    })?;

    let mut manifest: Vec<serde_json::Value> = Vec::new();
    let mut max_index: i64 = 0;

    for (i, (_track_id, _file_path, title, artist, album, duration_ms, artwork_id)) in tracks.into_iter().enumerate() {
        manifest.push(serde_json::json!({
            "track_id": _track_id,
            "title": title,
            "artist": artist,
            "album": album,
            "duration_ms": duration_ms,
            "artwork_id": if artwork_id.is_empty() { None } else { Some(artwork_id) },
        }));
        max_index = i as i64;
    }

    Ok(Json(serde_json::json!({
        "tracks": manifest,
        "total": manifest.len(),
        "cursor": max_index + 1,
    })))
}

#[derive(Debug, Deserialize)]
pub struct DeltaQuery {
    pub device_id: Option<Uuid>,
    pub cursor: Option<i64>,
    pub since: Option<String>,
    pub manifest_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DeltaEntry {
    pub track_id: Uuid,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub artwork_id: Option<String>,
}

pub async fn sync_manifest_delta_handler(
    State(state): State<AppState>,
    Query(query): Query<DeltaQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let all = michi_db::get_all_tracks_manifest(&state.db).await.map_err(|e| {
        v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string())
    })?;

    let total_count = all.len() as i64;
    let cursor = query.cursor.or(query.manifest_id).unwrap_or(0);

    let mut added: Vec<DeltaEntry> = Vec::new();
    for (i, (track_id, _file_path, title, artist, album, duration_ms, artwork_id)) in all.into_iter().enumerate() {
        let idx = i as i64;
        if idx >= cursor {
            added.push(DeltaEntry {
                track_id,
                title,
                artist,
                album,
                duration_ms,
                artwork_id: if artwork_id.is_empty() { None } else { Some(artwork_id) },
            });
        }
    }

    Ok(Json(serde_json::json!({
        "added": added,
        "deleted": [],
        "updated": [],
        "playlists_updated": false,
        "cursor": total_count,
        "total": total_count,
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
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
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
