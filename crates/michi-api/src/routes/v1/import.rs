use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use michi_core::ImportState;

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

const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
const MAX_SESSION_SIZE: u64 = 1024 * 1024 * 1024;
const ALLOWED_AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "aac", "m4a", "wav",
];

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

use lazy_static::lazy_static;

const MAX_IMPORT_SESSIONS: usize = 100;

lazy_static! {
    static ref IMPORT_SESSIONS: Arc<RwLock<HashMap<Uuid, ImportSessionState>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

fn sanitize_filename(filename: &str) -> String {
    let name = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
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

fn get_staging_dir(music_paths: &[std::path::PathBuf], cache_path: &std::path::Path) -> std::path::PathBuf {
    music_paths
        .first()
        .map(|p| p.join(".import"))
        .unwrap_or_else(|| cache_path.join("import_staging"))
}

fn get_session_dir(music_paths: &[std::path::PathBuf], cache_path: &std::path::Path, session_id: &Uuid) -> std::path::PathBuf {
    get_staging_dir(music_paths, cache_path).join(session_id.to_string())
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
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "total_tracks or total_playlists must be > 0",
        ));
    }
    if body.total_tracks > 10000 {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "TOO_MANY_TRACKS",
            "max 10000 tracks per session",
        ));
    }

    let db_session = michi_core::ImportSessionDb {
        session_id,
        device_id,
        total_tracks: body.total_tracks,
        total_playlists: body.total_playlists,
        imported_tracks: 0,
        imported_playlists: 0,
        total_size_bytes: 0,
        status: "created".into(),
        expires_at: expires_at.to_rfc3339(),
        created_at: Utc::now().to_rfc3339(),
    };

    michi_db::create_import_session(&state.db, &db_session)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Created, None)
        .await
        .ok();

    {
        let mut sessions = IMPORT_SESSIONS.write().await;
        if sessions.len() >= MAX_IMPORT_SESSIONS {
            return Err(v1_error(
                StatusCode::TOO_MANY_REQUESTS,
                "TOO_MANY_SESSIONS",
                "Too many active import sessions. Complete or cancel existing sessions first.",
            ));
        }
        sessions.insert(
            session_id,
            ImportSessionState {
                session_id,
                total_tracks: body.total_tracks,
                total_playlists: body.total_playlists,
                imported_tracks: 0,
                total_size_bytes: 0,
                device_id,
                seen_hashes: Vec::new(),
            },
        );
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
    headers: axum::http::HeaderMap,
    Json(body): Json<ImportUploadBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use base64::Engine;

    let session_state = {
        let sessions = IMPORT_SESSIONS.read().await;
        sessions.get(&session_id).cloned()
    }
    .ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            "import session not found or expired",
        )
    })?;

    if !is_allowed_extension(&body.filename) {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_EXTENSION",
            &format!(
                "extension not allowed. Accepted: {}",
                ALLOWED_AUDIO_EXTS.join(", ")
            ),
        ));
    }

    // Read X-Track-Id header if present (Player sends this)
    let local_track_id: Option<Uuid> = headers
        .get("X-Track-Id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok());

    // Read X-Checksum header if present
    let checksum_header = headers
        .get("X-Checksum")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let data = base64::engine::general_purpose::STANDARD
        .decode(&body.data)
        .map_err(|_| {
            v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_DATA",
                "invalid base64 data",
            )
        })?;

    if data.len() as u64 > MAX_FILE_SIZE {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "FILE_TOO_LARGE",
            &format!("file exceeds max size of {} bytes", MAX_FILE_SIZE),
        ));
    }
    if session_state.total_size_bytes + data.len() as u64 > MAX_SESSION_SIZE {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "SESSION_SIZE_EXCEEDED",
            &format!(
                "session exceeds max total size of {} bytes",
                MAX_SESSION_SIZE
            ),
        ));
    }

    let data_hash = compute_sha256(&data);

    // Prefer X-Checksum header if present, fall back to body.hash
    let expected_hash = checksum_header.as_ref().or(body.hash.as_ref());
    if let Some(hash) = expected_hash {
        if data_hash != *hash {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "HASH_MISMATCH",
                "SHA256 hash does not match data",
            ));
        }
    }

    if session_state.seen_hashes.contains(&data_hash) {
        return Ok(Json(serde_json::json!({
            "local_track_id": local_track_id,
            "status": "duplicate",
            "remote_track_id": null,
            "checksum": data_hash,
        })));
    }

    let safe_name = sanitize_filename(&body.filename);
    let import_dir = get_session_dir(&state.config.music_paths, &state.config.cache_path, &session_id);
    tokio::fs::create_dir_all(&import_dir).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &e.to_string(),
        )
    })?;

    let file_path = import_dir.join(&safe_name);
    if file_path.exists() {
        return Ok(Json(serde_json::json!({
            "local_track_id": local_track_id,
            "status": "duplicate",
            "remote_track_id": null,
            "checksum": data_hash,
        })));
    }

    tokio::fs::write(&file_path, &data).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &e.to_string(),
        )
    })?;

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Uploading, None)
        .await
        .ok();

    {
        let mut sessions = IMPORT_SESSIONS.write().await;
        if let Some(s) = sessions.get_mut(&session_id) {
            s.imported_tracks += 1;
            s.total_size_bytes += data.len() as u64;
            s.seen_hashes.push(data_hash.clone());
        }
    }

    michi_db::update_import_session_progress(&state.db, &session_id, 1, data.len() as u64)
        .await
        .ok();

    let ext = std::path::Path::new(&safe_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let remote_track_id = if ALLOWED_AUDIO_EXTS.contains(&ext.as_str()) {
        let metadata = tokio::task::spawn_blocking({
            let fp = file_path.clone();
            move || michi_metadata::read_metadata_safe(&fp)
        })
        .await
        .unwrap_or_default();
        let tid = michi_core::track_id_from_path(file_path.to_str().unwrap_or(""));
        let track = michi_core::Track {
            id: tid,
            title: metadata.title,
            artist: metadata.artist,
            album: metadata.album,
            album_artist: metadata.album_artist,
            duration_ms: metadata.duration_ms,
            file_path: file_path.to_string_lossy().to_string(),
            format: metadata.format,
            sample_rate: metadata.sample_rate,
            bit_depth: metadata.bit_depth,
            channels: metadata.channels,
            artwork_id: None,
            genre: metadata.genre,
            year: metadata.year,
            track_number: metadata.track_number,
            disc_number: metadata.disc_number,
            content_hash: Some(data_hash.clone()),
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        michi_db::upsert_track(&state.db, &track).await.ok();
        Some(tid)
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "local_track_id": local_track_id,
        "remote_track_id": remote_track_id,
        "status": "uploaded",
        "checksum": data_hash,
    })))
}

pub async fn import_preflight_handler(
    State(state): State<AppState>,
    Json(body): Json<michi_core::ImportPreflightRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut results: Vec<serde_json::Value> = Vec::new();
    for track in body.tracks {
        let local_track_id = track.local_track_id;
        let content_hash = track.content_hash.clone();
        let quick_hash = track.quick_hash.clone();
        let sha256_prefix = track.sha256_prefix.clone();
        let duration_ms = track.duration_ms;
        let title = track.title.clone();

        // Try exact match by full content_hash
        let exact_match = if let Some(ref h) = content_hash {
            michi_db::find_tracks_by_content_hash(&state.db, h)
                .await
                .ok()
                .and_then(|t| t.into_iter().next())
        } else {
            None
        };

        // Try quick_hash (first 16 hex chars) as fallback
        let quick_match = if exact_match.is_none() {
            if let Some(ref qh) = quick_hash {
                let all = michi_db::list_tracks(&state.db)
                    .await
                    .ok()
                    .unwrap_or_default();
                all.into_iter().find(|t| {
                    t.content_hash
                        .as_deref()
                        .map(|ch| ch.starts_with(qh))
                        .unwrap_or(false)
                })
            } else {
                None
            }
        } else {
            None
        };

        // Try sha256_prefix as legacy fallback
        let legacy_match = if exact_match.is_none() && quick_match.is_none() {
            if let Some(ref sp) = sha256_prefix {
                let all = michi_db::list_tracks(&state.db)
                    .await
                    .ok()
                    .unwrap_or_default();
                all.into_iter().find(|t| {
                    t.content_hash
                        .as_deref()
                        .map(|ch| ch.starts_with(sp))
                        .unwrap_or(false)
                })
            } else {
                None
            }
        } else {
            None
        };

        // Try metadata+duration as last resort
        let metadata_match =
            if exact_match.is_none() && quick_match.is_none() && legacy_match.is_none() {
                if let Some(ref ttl) = title {
                    if let Some(dur) = duration_ms {
                        let all = michi_db::list_tracks(&state.db)
                            .await
                            .ok()
                            .unwrap_or_default();
                        all.into_iter().find(|t| {
                            t.title.as_deref() == Some(ttl)
                                && t.duration_ms
                                    .map(|d| (d as i64 - dur as i64).abs() < 2000)
                                    .unwrap_or(false)
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

        // Check for partial conflict (same title, different duration)
        let conflict = if exact_match.is_none()
            && quick_match.is_none()
            && legacy_match.is_none()
            && metadata_match.is_none()
        {
            if let Some(ref ttl) = title {
                if let Some(dur) = duration_ms {
                    let all = michi_db::list_tracks(&state.db)
                        .await
                        .ok()
                        .unwrap_or_default();
                    all.into_iter().find(|t| {
                        t.title.as_deref() == Some(ttl)
                            && t.duration_ms
                                .map(|d| (d as i64 - dur as i64).abs() >= 2000)
                                .unwrap_or(false)
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let matched_track = exact_match
            .as_ref()
            .or(quick_match.as_ref())
            .or(legacy_match.as_ref())
            .or(metadata_match.as_ref());

        let (status, remote_track_id, match_type): (String, Option<Uuid>, String) =
            match matched_track {
                Some(t) if exact_match.is_some() => {
                    ("already_present".into(), Some(t.id), "exact_hash".into())
                }
                Some(t) if quick_match.is_some() => {
                    ("already_present".into(), Some(t.id), "quick_hash".into())
                }
                Some(t) if legacy_match.is_some() => {
                    ("already_present".into(), Some(t.id), "sha256_prefix".into())
                }
                Some(t) => (
                    "already_present".into(),
                    Some(t.id),
                    "metadata_duration".into(),
                ),
                None => match conflict.as_ref() {
                    Some(t) => ("conflict".into(), Some(t.id), "metadata_duration".into()),
                    None => ("needs_upload".into(), None, "none".into()),
                },
            };

        results.push(serde_json::json!({
            "local_track_id": local_track_id,
            "status": status,
            "remote_track_id": remote_track_id,
            "match": match_type,
        }));
    }

    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn import_session_status_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db_session = michi_db::get_import_session_full(&state.db, &session_id)
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
                "SESSION_NOT_FOUND",
                "import session not found",
            )
        })?;

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
    }
    .ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "SESSION_NOT_FOUND",
            "import session not found or expired",
        )
    })?;

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Committing, None)
        .await
        .ok();

    let staging_dir = get_session_dir(&state.config.music_paths, &state.config.cache_path, &session_id);
    let final_dir = state
        .config
        .music_paths
        .first()
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

    let concurrency = state.config.resource_profile.scan_concurrency();
    let tracks = michi_scanner::scan_directories_with_concurrency(&[final_dir], concurrency).await;
    let _scanned_count = tracks.len();

    // Check for unresolved conflicts: same content_hash but different duration_ms already in library
    let has_conflicts = {
        let mut conflict = false;
        for track in &tracks {
            if let Some(ref hash) = track.content_hash {
                let existing = michi_db::find_tracks_by_content_hash(&state.db, hash)
                    .await
                    .ok()
                    .unwrap_or_default();
                if existing.iter().any(|t| {
                    t.id != track.id
                        && t.duration_ms
                            .map(|d| {
                                (d as i64 - track.duration_ms.unwrap_or(0) as i64).abs() > 2000
                            })
                            .unwrap_or(false)
                }) {
                    conflict = true;
                    break;
                }
            }
        }
        conflict
    };

    if has_conflicts {
        michi_db::set_import_session_status(
            &state.db,
            &session_id,
            &ImportState::Failed,
            Some("unresolved conflicts"),
        )
        .await
        .ok();
        return Err(v1_error(StatusCode::CONFLICT, "UNRESOLVED_CONFLICTS",
            "Import has duration conflicts with existing tracks. Rollback and fix metadata before retrying."));
    }

    michi_db::upsert_tracks(&state.db, &tracks).await.ok();
    cleanup_session_dir(&staging_dir).await;

    // Build mapping with per-track status
    let mut mapping: Vec<serde_json::Value> = Vec::new();
    for track in &tracks {
        let existing_by_hash = if let Some(ref h) = track.content_hash {
            michi_db::find_tracks_by_content_hash(&state.db, h)
                .await
                .ok()
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let (status, remote_id): (String, Uuid) =
            if existing_by_hash.iter().any(|t| t.id == track.id) {
                // This exact track was just inserted
                ("inserted".into(), track.id)
            } else if existing_by_hash.iter().any(|t| t.id != track.id) {
                // Hash matched a different track — merged
                let existing_id = existing_by_hash.first().map(|t| t.id).unwrap_or(track.id);
                ("merged".into(), existing_id)
            } else {
                ("inserted".into(), track.id)
            };

        mapping.push(serde_json::json!({
            "local_track_id": track.id,
            "status": status,
            "remote_track_id": remote_id,
            "checksum": track.content_hash,
        }));
    }

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Committed, None)
        .await
        .ok();
    michi_db::close_import_session(&state.db, &session_id)
        .await
        .ok();
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
    let staging_dir = get_session_dir(&state.config.music_paths, &state.config.cache_path, &session_id);
    cleanup_session_dir(&staging_dir).await;
    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::RolledBack, None)
        .await
        .ok();
    michi_db::close_import_session(&state.db, &session_id)
        .await
        .ok();
    Json(serde_json::json!({ "status": "rolled_back" }))
}

pub fn spawn_import_cleanup(config: &michi_config::Config, db: sqlx::SqlitePool) {
    let music_paths = config.music_paths.clone();
    let cache_path = config.cache_path.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let cutoff = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
            if let Ok(expired) = michi_db::list_expired_import_sessions(&db, &cutoff).await {
                for sid in expired {
                    michi_db::expire_import_session(&db, &sid).await.ok();
                    let dir = get_session_dir(&music_paths, &cache_path, &sid);
                    cleanup_session_dir(&dir).await;
                }
            }
            // Also clean old staging dirs with no DB record
            let staging = get_staging_dir(&music_paths, &cache_path);
            if staging.exists() {
                if let Ok(mut entries) = tokio::fs::read_dir(&staging).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        if entry.path().is_dir() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if let Ok(uid) = Uuid::parse_str(&name) {
                                if michi_db::get_import_session_full(&db, &uid)
                                    .await
                                    .ok()
                                    .flatten()
                                    .is_none()
                                {
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
