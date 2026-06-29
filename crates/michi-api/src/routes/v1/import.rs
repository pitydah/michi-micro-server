use axum::{extract::{Path, State}, http::StatusCode, Json};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use michi_core::ImportState;

fn v1_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({
        "error": { "code": code, "message": message, "details": {} }
    })))
}

const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
const MAX_SESSION_SIZE: u64 = 1024 * 1024 * 1024;
const ALLOWED_AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "aac", "m4a", "wav", "aiff", "dsf", "dff"];

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

fn is_allowed_extension(filename: &str) -> bool {
    std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| ALLOWED_AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn get_staging_dir(music_paths: &[std::path::PathBuf]) -> std::path::PathBuf {
    music_paths.first()
        .map(|p| p.join(".import"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/michi-import"))
}

fn get_session_dir(music_paths: &[std::path::PathBuf], session_id: &Uuid) -> std::path::PathBuf {
    get_staging_dir(music_paths).join(session_id.to_string())
}

async fn cleanup_session_dir(path: &std::path::Path) {
    if path.exists() {
        let _ = tokio::fs::remove_dir_all(path).await;
    }
}

pub async fn import_session_handler(
    State(state): State<AppState>,
    Json(body): Json<ImportSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = Uuid::new_v4();
    let expires_at = Utc::now() + chrono::Duration::hours(1);
    let device_id = Uuid::nil();

    if body.total_tracks == 0 && body.total_playlists == 0 {
        return Err(v1_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST", "total_tracks or total_playlists must be > 0"));
    }
    if body.total_tracks > 10000 {
        return Err(v1_error(StatusCode::BAD_REQUEST, "TOO_MANY_TRACKS", "max 10000 tracks per session"));
    }

    let db_session = michi_core::ImportSessionDb {
        session_id, device_id,
        total_tracks: body.total_tracks, total_playlists: body.total_playlists,
        imported_tracks: 0, imported_playlists: 0, total_size_bytes: 0,
        status: "created".into(),
        expires_at: expires_at.to_rfc3339(),
        created_at: Utc::now().to_rfc3339(),
    };

    michi_db::create_import_session(&state.db, &db_session).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;
    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Created, None).await.ok();

    {
        let mut sessions = IMPORT_SESSIONS.write().await;
        sessions.insert(session_id, ImportSessionState {
            session_id, total_tracks: body.total_tracks, total_playlists: body.total_playlists,
            imported_tracks: 0, total_size_bytes: 0, device_id, seen_hashes: Vec::new(),
        });
    }

    Ok(Json(serde_json::json!({
        "session_id": session_id, "expires_at": expires_at.to_rfc3339(),
        "max_chunk_size": 10485760, "allowed_extensions": ALLOWED_AUDIO_EXTS,
        "max_file_size": MAX_FILE_SIZE,
    })))
}

pub async fn import_upload_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(body): Json<ImportUploadBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use base64::Engine;

    let session_state = {
        let sessions = IMPORT_SESSIONS.read().await;
        sessions.get(&session_id).cloned()
    }.ok_or_else(|| {
        v1_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", "import session not found or expired")
    })?;

    if !is_allowed_extension(&body.filename) {
        return Err(v1_error(StatusCode::BAD_REQUEST, "INVALID_EXTENSION", &format!(
            "extension not allowed. Accepted: {}", ALLOWED_AUDIO_EXTS.join(", ")
        )));
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(&body.data)
        .map_err(|_| v1_error(StatusCode::BAD_REQUEST, "INVALID_DATA", "invalid base64 data"))?;

    if data.len() as u64 > MAX_FILE_SIZE {
        return Err(v1_error(StatusCode::BAD_REQUEST, "FILE_TOO_LARGE", &format!(
            "file exceeds max size of {} bytes", MAX_FILE_SIZE
        )));
    }
    if session_state.total_size_bytes + data.len() as u64 > MAX_SESSION_SIZE {
        return Err(v1_error(StatusCode::BAD_REQUEST, "SESSION_SIZE_EXCEEDED", &format!(
            "session exceeds max total size of {} bytes", MAX_SESSION_SIZE
        )));
    }

    let data_hash = compute_sha256(&data);
    if let Some(ref hash) = body.hash {
        if data_hash != *hash {
            return Err(v1_error(StatusCode::BAD_REQUEST, "HASH_MISMATCH", "SHA256 hash does not match data"));
        }
    }

    if session_state.seen_hashes.contains(&data_hash) {
        return Ok(Json(serde_json::json!({
            "accepted": false, "is_duplicate": true, "track_id": null,
        })));
    }

    let safe_name = sanitize_filename(&body.filename);
    let import_dir = get_session_dir(&state.config.music_paths, &session_id);
    tokio::fs::create_dir_all(&import_dir).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR", &e.to_string()))?;

    let file_path = import_dir.join(&safe_name);
    if file_path.exists() {
        return Ok(Json(serde_json::json!({
            "accepted": false, "is_duplicate": true, "track_id": null,
        })));
    }

    tokio::fs::write(&file_path, &data).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR", &e.to_string()))?;

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Uploading, None).await.ok();

    {
        let mut sessions = IMPORT_SESSIONS.write().await;
        if let Some(s) = sessions.get_mut(&session_id) {
            s.imported_tracks += 1;
            s.total_size_bytes += data.len() as u64;
            s.seen_hashes.push(data_hash.clone());
        }
    }

    michi_db::update_import_session_progress(&state.db, &session_id, 1, data.len() as u64).await.ok();

    let ext = std::path::Path::new(&safe_name)
        .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    let track_id = if ALLOWED_AUDIO_EXTS.contains(&ext.as_str()) {
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
            content_hash: Some(data_hash.clone()),
            created_at: Utc::now(), updated_at: Utc::now(),
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

pub async fn import_preflight_handler(
    State(state): State<AppState>,
    Json(body): Json<michi_core::ImportPreflightRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut results = Vec::new();
    for identity in body.tracks {
        let existing = michi_db::find_tracks_by_content_hash(&state.db, &identity.content_hash).await
            .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

        let (status, track_id) = if existing.is_empty() {
            ("needs_upload".to_string(), None)
        } else if existing.iter().any(|t| {
            t.duration_ms.map(|d| d as i64) == identity.duration_ms.map(|d| d as i64)
        }) {
            ("already_present".to_string(), existing.first().map(|t| t.id))
        } else {
            ("conflict".to_string(), existing.first().map(|t| t.id))
        };

        results.push(michi_core::ImportPreflightItem {
            hash: identity.content_hash,
            status,
            local_track_id: track_id,
        });
    }

    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn import_session_status_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db_session = michi_db::get_import_session_full(&state.db, &session_id).await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND", "import session not found"))?;

    Ok(Json(serde_json::json!({
        "session_id": db_session.session_id,
        "status": db_session.status,
        "total_tracks": db_session.total_tracks,
        "total_playlists": db_session.total_playlists,
        "imported_tracks": db_session.imported_tracks,
        "total_size_bytes": db_session.total_size_bytes,
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

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Committing, None).await.ok();

    let staging_dir = get_session_dir(&state.config.music_paths, &session_id);
    let final_dir = state.config.music_paths.first()
        .cloned()
        .unwrap_or_else(|| staging_dir.clone());

    if staging_dir.exists() {
        if let Ok(mut entries) = tokio::fs::read_dir(&staging_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let src = entry.path();
                if src.is_file() {
                    let dest = final_dir.join(src.file_name().unwrap());
                    if !dest.exists() {
                        let _ = tokio::fs::copy(&src, &dest).await;
                    }
                }
            }
        }
    }

    let tracks = michi_scanner::scan_directories(&[final_dir]).await;
    michi_db::upsert_tracks(&state.db, &tracks).await.ok();
    cleanup_session_dir(&staging_dir).await;

    // Build mapping (non-async, fetch content_hash data up front)
    let mut tracks_with_hashes: Vec<(Uuid, Option<String>)> = Vec::new();
    for track in &tracks {
        tracks_with_hashes.push((track.id, track.content_hash.clone()));
    }
    drop(tracks);

    let mut mapping: Vec<serde_json::Value> = Vec::new();
    for (local_id, content_hash) in &tracks_with_hashes {
        let existing_by_hash = if let Some(ref h) = content_hash {
            michi_db::find_tracks_by_content_hash(&state.db, h).await.ok().unwrap_or_default()
        } else {
            Vec::new()
        };
        let matched = existing_by_hash.iter().any(|t| t.id != *local_id);
        let existing_id = if matched { existing_by_hash.first().map(|t| t.id) } else { None };
        mapping.push(serde_json::json!({
            "local_track_id": local_id,
            "hash": content_hash,
            "matched": matched,
            "existing_track_id": existing_id,
        }));
    }

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Committed, None).await.ok();
    michi_db::close_import_session(&state.db, &session_id).await.ok();
    let _ = state.tx.send(r#"{"type":"library_updated"}"#.to_string());

    Ok(Json(serde_json::json!({
        "tracks_imported": _session_state.imported_tracks,
        "playlists_imported": 0,
        "total_size_bytes": _session_state.total_size_bytes,
        "mapping": mapping,
    })))
}

pub async fn import_rollback_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Json<serde_json::Value> {
    IMPORT_SESSIONS.write().await.remove(&session_id);
    let staging_dir = get_session_dir(&state.config.music_paths, &session_id);
    cleanup_session_dir(&staging_dir).await;
    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::RolledBack, None).await.ok();
    michi_db::close_import_session(&state.db, &session_id).await.ok();
    Json(serde_json::json!({ "status": "rolled_back" }))
}

/// Background job to clean up expired import sessions and stale staging dirs
pub fn spawn_import_cleanup(config: &michi_config::Config, db: sqlx::SqlitePool) {
    let music_paths = config.music_paths.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let cutoff = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
            if let Ok(expired) = michi_db::list_expired_import_sessions(&db, &cutoff).await {
                for sid in expired {
                    michi_db::expire_import_session(&db, &sid).await.ok();
                    let dir = get_session_dir(&music_paths, &sid);
                    cleanup_session_dir(&dir).await;
                }
            }
            // Also clean old staging dirs with no DB record
            let staging = get_staging_dir(&music_paths);
            if staging.exists() {
                if let Ok(mut entries) = tokio::fs::read_dir(&staging).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        if entry.path().is_dir() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if let Ok(uid) = Uuid::parse_str(&name) {
                                if michi_db::get_import_session_full(&db, &uid).await.ok().flatten().is_none() {
                                    cleanup_session_dir(&entry.path()).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}
