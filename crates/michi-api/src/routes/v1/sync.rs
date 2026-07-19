use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
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

// ── Upload endpoints ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UploadInitBody {
    pub filename: String,
    pub original_path: String,
    pub file_size: i64,
    pub expected_hash: String,
    pub uploaded_by: String,
}

pub async fn sync_upload_init_handler(
    State(state): State<AppState>,
    Json(body): Json<UploadInitBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Check if file already exists by hash
    if let Ok(Some(existing)) = state
        .sync_manager
        .check_file_exists(&body.expected_hash)
        .await
    {
        return Ok(Json(serde_json::json!({
            "status": "exists",
            "file_id": existing.id,
            "filename": existing.filename,
        })));
    }

    let init = michi_sync::UploadInit {
        filename: body.filename,
        original_path: body.original_path,
        file_size: body.file_size,
        expected_hash: body.expected_hash,
        uploaded_by: body.uploaded_by,
    };

    let file_id = state.sync_manager.init_upload(init).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UPLOAD_INIT_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "initialized",
        "file_id": file_id,
    })))
}

pub async fn sync_upload_chunk_handler(
    State(state): State<AppState>,
    Path(file_id): Path<Uuid>,
    Json(chunk): Json<michi_sync::UploadChunk>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if chunk.file_id != file_id {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "FILE_ID_MISMATCH",
            "chunk file_id does not match path",
        ));
    }

    let progress = state
        .sync_manager
        .upload_chunk(chunk)
        .await
        .map_err(|e| match &e {
            michi_sync::SyncError::HashMismatch { .. } => {
                v1_error(StatusCode::BAD_REQUEST, "HASH_MISMATCH", &e.to_string())
            }
            _ => v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UPLOAD_CHUNK_ERROR",
                &e.to_string(),
            ),
        })?;

    Ok(Json(serde_json::json!({
        "status": if progress.completed { "completed" } else { "in_progress" },
        "progress": progress,
    })))
}

pub async fn sync_upload_status_handler(
    State(state): State<AppState>,
    Path(file_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let progress = state
        .sync_manager
        .get_upload_progress(&file_id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UPLOAD_STATUS_ERROR",
                &e.to_string(),
            )
        })?;

    match progress {
        Some(p) => Ok(Json(serde_json::json!({
            "status": if p.completed { "completed" } else { "in_progress" },
            "progress": p,
        }))),
        None => Ok(Json(serde_json::json!({
            "status": "not_found",
        }))),
    }
}

// ── Simple file upload (single POST, base64) ─────────────────────

#[derive(Debug, Deserialize)]
pub struct UploadFileBody {
    pub filename: String,
    pub original_path: String,
    pub uploaded_by: String,
    pub data_base64: String,
}

pub async fn sync_upload_file_handler(
    State(state): State<AppState>,
    Json(body): Json<UploadFileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use base64::Engine;

    let data = base64::engine::general_purpose::STANDARD
        .decode(&body.data_base64)
        .map_err(|e| {
            v1_error(
                StatusCode::BAD_REQUEST,
                "BASE64_DECODE_ERROR",
                &e.to_string(),
            )
        })?;

    let file_id = uuid::Uuid::new_v4();
    let server_path = state
        .config
        .cache_path
        .join("uploads")
        .join(file_id.to_string());
    let _ = std::fs::create_dir_all(server_path.parent().unwrap());

    tokio::fs::write(&server_path, &data).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "WRITE_ERROR",
            &e.to_string(),
        )
    })?;

    let hash = state
        .sync_manager
        .calculate_file_hash(&server_path)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "HASH_ERROR",
                &e.to_string(),
            )
        })?;

    // Check dedup
    if let Ok(Some(existing)) = state.sync_manager.check_file_exists(&hash).await {
        let _ = tokio::fs::remove_file(&server_path).await;
        return Ok(Json(serde_json::json!({
            "status": "exists",
            "file_id": existing.id,
            "filename": existing.filename,
            "hash": hash,
        })));
    }

    let file_id = state
        .sync_manager
        .register_uploaded_file(
            body.filename,
            body.original_path,
            server_path.to_string_lossy().to_string(),
            hash.clone(),
            data.len() as i64,
            body.uploaded_by,
        )
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REGISTER_ERROR",
                &e.to_string(),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "uploaded",
        "file_id": file_id,
        "hash": hash,
        "size_bytes": data.len(),
    })))
}

// ── Playlist sync endpoint ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SyncPlaylistBody {
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<String>,
}

pub async fn sync_playlist_handler(
    State(state): State<AppState>,
    Json(body): Json<SyncPlaylistBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "playlist name is required",
        ));
    }

    let mut valid_tracks = Vec::new();
    let mut missing_tracks = Vec::new();

    for tid_str in &body.tracks {
        if let Ok(tid) = Uuid::parse_str(tid_str) {
            match michi_db::get_track(&state.db, &tid).await {
                Ok(Some(_)) => valid_tracks.push(tid),
                _ => missing_tracks.push(tid_str.clone()),
            }
        } else {
            missing_tracks.push(tid_str.clone());
        }
    }

    let input = michi_core::PlaylistCreate {
        name: body.name.trim().to_string(),
        description: body.description,
    };

    let playlist = michi_db::create_playlist(&state.db, &input, None)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    for tid in &valid_tracks {
        let _ = michi_db::add_track_to_playlist(&state.db, &playlist.id, tid).await;
    }

    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());

    Ok(Json(serde_json::json!({
        "status": "ok",
        "playlist": playlist,
        "tracks_added": valid_tracks.len(),
        "tracks_missing": missing_tracks,
    })))
}

// ── Existing sync endpoints ───────────────────────────────────────

pub async fn sync_manifest_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::get_all_tracks_manifest(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let mut manifest: Vec<serde_json::Value> = Vec::new();
    let mut max_index: i64 = 0;

    for (i, (_track_id, _file_path, title, artist, album, duration_ms, artwork_id)) in
        tracks.into_iter().enumerate()
    {
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
    let all = michi_db::get_all_tracks_manifest(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let total_count = all.len() as i64;
    let cursor = query.cursor.or(query.manifest_id).unwrap_or(0);

    let mut added: Vec<DeltaEntry> = Vec::new();
    for (i, (track_id, _file_path, title, artist, album, duration_ms, artwork_id)) in
        all.into_iter().enumerate()
    {
        let idx = i as i64;
        if idx >= cursor {
            added.push(DeltaEntry {
                track_id,
                title,
                artist,
                album,
                duration_ms,
                artwork_id: if artwork_id.is_empty() {
                    None
                } else {
                    Some(artwork_id)
                },
            });
        }
    }

    let mut deleted: Vec<String> = Vec::new();
    let mut updated: Vec<String> = Vec::new();
    if let Some(since) = query.since.as_ref() {
        if let Ok(changes) = sqlx::query_as::<_, (String, String, String)>(
            "SELECT entity_id, action, created_at FROM change_journal WHERE created_at > ? ORDER BY created_at ASC LIMIT 500",
        )
        .bind(since)
        .fetch_all(&state.db)
        .await
        {
            for (entity_id, action, _created_at) in changes {
                match action.as_str() {
                    "delete" => deleted.push(entity_id),
                    "upsert" => updated.push(entity_id),
                    _ => {}
                }
            }
        }
    }

    Ok(Json(serde_json::json!({
        "added": added,
        "deleted": deleted,
        "updated": updated,
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
        playlist_id: None,
        queue_position: None,
        device_id: None,
    };

    {
        let mut current = state.playback_state.write().await;
        *current = new_state.clone();
    }

    let _ = state.sync_tx.send(new_state.into());
    let _ = state.tx.send(
        serde_json::json!({
            "type": "sync_state",
            "track_id": body.track_id,
            "position_ms": body.position_ms,
            "playing": body.playing,
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
