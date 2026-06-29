use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

fn v1_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({
        "error": { "code": code, "message": message, "details": {} }
    })))
}

#[derive(Debug, Deserialize)]
pub struct ImportSessionRequest {
    pub total_tracks: u32,
    pub total_playlists: u32,
}

#[derive(Debug, Deserialize)]
pub struct ImportUploadBody {
    pub filename: String,
    pub hash: Option<String>,
    pub data: String,
}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ImportSessionState {
    pub session_id: Uuid,
    pub total_tracks: u32,
    pub total_playlists: u32,
    pub imported_tracks: u32,
    pub total_size_bytes: u64,
    pub device_id: Uuid,
    pub seen_hashes: Vec<String>,
}

lazy_static::lazy_static! {
    static ref IMPORT_SESSIONS: Arc<RwLock<HashMap<Uuid, ImportSessionState>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

fn sanitize_filename(filename: &str) -> String {
    let name = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn get_import_dir(cache_path: &std::path::Path, session_id: &Uuid) -> std::path::PathBuf {
    cache_path.join("import").join(session_id.to_string())
}

pub async fn import_session_handler(
    State(state): State<AppState>,
    Json(body): Json<ImportSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = Uuid::new_v4();
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);

    let db_session = michi_core::ImportSessionDb {
        session_id,
        device_id: Uuid::nil(),
        total_tracks: body.total_tracks,
        total_playlists: body.total_playlists,
        imported_tracks: 0, imported_playlists: 0, total_size_bytes: 0,
        status: "active".into(),
        expires_at: expires_at.to_rfc3339(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    michi_db::create_import_session(&state.db, &db_session).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

    {
        let mut sessions = IMPORT_SESSIONS.write().await;
        sessions.insert(session_id, ImportSessionState {
            session_id, total_tracks: body.total_tracks, total_playlists: body.total_playlists,
            imported_tracks: 0, total_size_bytes: 0, device_id: Uuid::nil(), seen_hashes: Vec::new(),
        });
    }

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "expires_at": expires_at.to_rfc3339(),
        "max_chunk_size": 10485760,
    })))
}

pub async fn import_upload_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(body): Json<ImportUploadBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use base64::Engine;

    let data = base64::engine::general_purpose::STANDARD
        .decode(&body.data)
        .map_err(|_| v1_error(StatusCode::BAD_REQUEST, "INVALID_DATA", "invalid base64 data"))?;

    let data_hash = compute_sha256(&data);

    if let Some(ref hash) = body.hash {
        if data_hash != *hash {
            return Err(v1_error(StatusCode::BAD_REQUEST, "HASH_MISMATCH", "SHA256 hash does not match data"));
        }
    }

    let is_duplicate_by_hash = {
        let sessions = IMPORT_SESSIONS.read().await;
        sessions.get(&session_id).map(|s| s.seen_hashes.contains(&data_hash)).unwrap_or(false)
    };

    if is_duplicate_by_hash {
        return Ok(Json(serde_json::json!({
            "accepted": false, "is_duplicate": true, "track_id": null,
        })));
    }

    let safe_name = sanitize_filename(&body.filename);
    let import_dir = get_import_dir(&state.config.cache_path, &session_id);
    tokio::fs::create_dir_all(&import_dir).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR", &e.to_string()))?;

    let file_path = import_dir.join(&safe_name);

    let is_duplicate_by_name = file_path.exists();
    if is_duplicate_by_name {
        return Ok(Json(serde_json::json!({
            "accepted": false, "is_duplicate": true, "track_id": null,
        })));
    }

    tokio::fs::write(&file_path, &data).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR", &e.to_string()))?;

    {
        let mut sessions = IMPORT_SESSIONS.write().await;
        if let Some(s) = sessions.get_mut(&session_id) {
            s.imported_tracks += 1;
            s.total_size_bytes += data.len() as u64;
            s.seen_hashes.push(data_hash);
        }
    }

    michi_db::update_import_session_progress(&state.db, &session_id, 1, data.len() as u64).await.ok();

    let ext = std::path::Path::new(&safe_name)
        .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let audio_exts = ["mp3", "flac", "ogg", "opus", "aac", "m4a", "wav", "aiff", "dsf", "dff"];

    let track_id = if audio_exts.contains(&ext.as_str()) {
        let metadata = tokio::task::spawn_blocking({
            let fp = file_path.clone();
            move || michi_metadata::read_metadata_safe(&fp)
        }).await.unwrap_or_default();

        let tid = michi_core::track_id_from_path(file_path.to_str().unwrap_or(""));
        let track = michi_core::Track {
            id: tid, title: metadata.title, artist: metadata.artist,
            album: metadata.album, album_artist: metadata.album_artist,
            duration_ms: metadata.duration_ms,
            file_path: file_path.to_string_lossy().to_string(),
            format: metadata.format, sample_rate: metadata.sample_rate,
            bit_depth: metadata.bit_depth, channels: metadata.channels,
            artwork_id: None, genre: metadata.genre, year: metadata.year,
            track_number: metadata.track_number, disc_number: metadata.disc_number,
            created_at: chrono::Utc::now(), updated_at: chrono::Utc::now(),
        };
        michi_db::upsert_track(&state.db, &track).await.ok();
        Some(tid)
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "accepted": true, "is_duplicate": false, "track_id": track_id,
    })))
}

pub async fn import_commit_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<serde_json::value::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _session_state = {
        let mut sessions = IMPORT_SESSIONS.write().await;
        sessions.remove(&session_id)
    }.ok_or_else(|| {
        v1_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", "import session not found or expired")
    })?;

    michi_db::close_import_session(&state.db, &session_id).await.ok();
    let _ = state.tx.send(r#"{"type":"library_updated"}"#.to_string());

    let import_dir = get_import_dir(&state.config.cache_path, &session_id);
    if import_dir.exists() {
        let tracks = michi_scanner::scan_directories(&[import_dir]).await;
        michi_db::upsert_tracks(&state.db, &tracks).await.ok();
    }

    Ok(Json(serde_json::json!({
        "tracks_imported": _session_state.imported_tracks,
        "playlists_imported": 0,
        "total_size_bytes": _session_state.total_size_bytes,
    })))
}
