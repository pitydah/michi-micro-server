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
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }
    // Check if file already exists by hash
    match state
        .sync_manager
        .check_file_exists(&body.expected_hash)
        .await
    {
        Ok(Some(existing)) => {
            return Ok(Json(serde_json::json!({
                "status": "exists",
                "file_id": existing.id,
                "filename": existing.filename,
            })));
        }
        Ok(None) => {}
        Err(e) => {
            return Err(v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            ));
        }
    }

    let init = michi_sync::UploadInit {
        filename: body.filename,
        original_path: body.original_path,
        file_size: body.file_size,
        expected_hash: body.expected_hash,
        uploaded_by: body.uploaded_by,
    };

    let file_id = state
        .sync_manager
        .init_upload(init)
        .await
        .map_err(|e| match &e {
            michi_sync::SyncError::InvalidChunkParameter(_) => {
                v1_error(StatusCode::BAD_REQUEST, "INVALID_PARAMETER", &e.to_string())
            }
            michi_sync::SyncError::DatabaseError(_) => v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            ),
            _ => v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UPLOAD_INIT_ERROR",
                &e.to_string(),
            ),
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
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }
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
            michi_sync::SyncError::SessionNotFound(_) => {
                v1_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", &e.to_string())
            }
            michi_sync::SyncError::ChunkConflict { .. } => {
                v1_error(StatusCode::CONFLICT, "CHUNK_CONFLICT", &e.to_string())
            }
            michi_sync::SyncError::UploadAlreadyCompleted(_) => {
                v1_error(StatusCode::CONFLICT, "ALREADY_COMPLETED", &e.to_string())
            }
            michi_sync::SyncError::UploadCancelled(_) => {
                v1_error(StatusCode::GONE, "UPLOAD_CANCELLED", &e.to_string())
            }
            michi_sync::SyncError::InvalidChunkParameter(_) => {
                v1_error(StatusCode::BAD_REQUEST, "INVALID_PARAMETER", &e.to_string())
            }
            michi_sync::SyncError::HashMismatch { .. } => {
                v1_error(StatusCode::BAD_REQUEST, "HASH_MISMATCH", &e.to_string())
            }
            michi_sync::SyncError::DatabaseError(_) => v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            ),
            _ => v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UPLOAD_CHUNK_ERROR",
                &e.to_string(),
            ),
        })?;

    Ok(Json(serde_json::json!({
        "status": progress.status.as_db_str(),
        "progress": progress,
    })))
}

pub async fn sync_upload_status_handler(
    State(state): State<AppState>,
    Path(file_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }
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
            "status": p.status.as_db_str(),
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
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }
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

    if let Some(parent) = server_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DIRECTORY_CREATE_ERROR",
                &e.to_string(),
            )
        })?;
    }

    tokio::fs::write(&server_path, &data).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "WRITE_ERROR",
            &e.to_string(),
        )
    })?;

    let hash = match state.sync_manager.calculate_file_hash(&server_path).await {
        Ok(h) => h,
        Err(e) => {
            let _ = tokio::fs::remove_file(&server_path).await;
            return Err(v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "HASH_ERROR",
                &e.to_string(),
            ));
        }
    };

    // Check dedup
    match state.sync_manager.check_file_exists(&hash).await {
        Ok(Some(existing)) => {
            if let Err(e) = tokio::fs::remove_file(&server_path).await {
                tracing::warn!(path = ?server_path, error = %e, "failed to clean up deduplicated staging file");
            }
            return Ok(Json(serde_json::json!({
                "status": "exists",
                "file_id": existing.id,
                "filename": existing.filename,
                "hash": hash,
            })));
        }
        Ok(None) => {}
        Err(e) => {
            let _ = tokio::fs::remove_file(&server_path).await;
            return Err(v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            ));
        }
    }

    let file_id = match state
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
    {
        Ok(id) => id,
        Err(e) => {
            let _ = tokio::fs::remove_file(&server_path).await;
            return Err(v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REGISTER_ERROR",
                &e.to_string(),
            ));
        }
    };

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
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }
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
                Ok(None) => missing_tracks.push(tid_str.clone()),
                Err(e) => {
                    return Err(v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &e.to_string(),
                    ));
                }
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

    let mut tracks_added = 0;
    let mut tracks_failed = Vec::new();
    for tid in &valid_tracks {
        match michi_db::add_track_to_playlist(&state.db, &playlist.id, tid).await {
            Ok(_) => tracks_added += 1,
            Err(e) => {
                tracing::warn!(playlist_id = %playlist.id, track_id = %tid, error = %e, "failed to add track to playlist during sync");
                tracks_failed.push(serde_json::json!({
                    "track_id": tid.to_string(),
                    "error": e.to_string(),
                }));
            }
        }
    }

    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());

    let is_partial = !missing_tracks.is_empty() || tracks_added != valid_tracks.len();
    let status_str = if is_partial { "partial" } else { "ok" };

    Ok(Json(serde_json::json!({
        "status": status_str,
        "playlist": playlist,
        "tracks_added": tracks_added,
        "tracks_missing": missing_tracks,
        "tracks_failed": tracks_failed,
    })))
}

// ── Existing sync endpoints ───────────────────────────────────────

pub async fn sync_manifest_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }
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
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }
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
        let changes = sqlx::query_as::<_, (String, String, String)>(
            "SELECT entity_id, action, created_at FROM change_journal WHERE created_at > ? ORDER BY created_at ASC LIMIT 500",
        )
        .bind(since)
        .fetch_all(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

        for (entity_id, action, _created_at) in changes {
            match action.as_str() {
                "delete" => deleted.push(entity_id),
                "upsert" => updated.push(entity_id),
                _ => {}
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
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub event_id: Option<Uuid>,
    #[serde(default)]
    pub sequence: Option<u64>,
    #[serde(default)]
    pub epoch: Option<u64>,
    #[serde(default)]
    pub boot_id: Option<Uuid>,
}

pub async fn sync_state_handler(
    State(state): State<AppState>,
    Json(body): Json<SyncStateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("sync") {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODULE_DISABLED",
            "sync module is disabled",
        ));
    }

    let (device_id, event_id, sequence, epoch, boot_id) = match body.device_id {
        Some(dev) => (
            Some(dev),
            Some(body.event_id.unwrap_or_else(Uuid::new_v4)),
            body.sequence,
            body.epoch,
            body.boot_id,
        ),
        None => (
            Some(state.server_id().to_string()),
            Some(body.event_id.unwrap_or_else(Uuid::new_v4)),
            Some(state.playback_projection.next_local_sequence()),
            Some(state.playback_projection.server_epoch()),
            Some(state.playback_projection.boot_id()),
        ),
    };

    let peer_state = michi_sync::PlaybackState {
        track_id: body.track_id,
        position_ms: body.position_ms,
        playing: body.playing,
        volume: body.volume,
        updated_at: chrono::Utc::now(),
        playlist_id: None,
        queue_position: None,
        device_id,
        shuffle: false,
        repeat: "off".into(),
        event_id,
        sequence,
        epoch,
        boot_id,
    };

    let _ = state.sync_tx.send(peer_state.into());
    let _ = state.tx.send(
        serde_json::json!({
            "type": "sync_state",
            "track_id": body.track_id,
            "position_ms": body.position_ms,
            "playing": body.playing,
        })
        .to_string(),
    );

    // Return authoritative local server state for confirmation
    let local_snap = state.playback_engine.snapshot().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ENGINE_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "received",
        "server_playback": {
            "playing": local_snap.is_playing(),
            "track_id": local_snap.track_id,
            "position_ms": local_snap.position_ms,
            "lifecycle": local_snap.lifecycle.as_str(),
        }
    })))
}
