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
const ALLOWED_AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "aac", "m4a", "wav"];

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportFile {
    pub local_track_id: Option<Uuid>,
    pub filename: String,
    pub safe_name: String,
    pub staging_filename: String,
    pub checksum: String,
    pub size_bytes: u64,
    pub remote_track_id: Uuid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportSessionState {
    pub session_id: Uuid,
    pub total_tracks: u32,
    pub total_playlists: u32,
    pub imported_tracks: u32,
    pub total_size_bytes: u64,
    pub device_id: Uuid,
    pub seen_hashes: Vec<String>,
    pub files: Vec<ImportFile>,
}

impl ImportSessionState {
    pub async fn save_manifest(&self, session_dir: &std::path::Path) {
        let manifest_path = session_dir.join("manifest.json");
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let tmp_path = session_dir.join("manifest.json.tmp");
            if tokio::fs::write(&tmp_path, data.as_bytes()).await.is_ok() {
                let _ = tokio::fs::rename(&tmp_path, &manifest_path).await;
            }
        }
    }

    pub async fn load_manifest(session_dir: &std::path::Path) -> Option<Self> {
        let manifest_path = session_dir.join("manifest.json");
        if let Ok(data) = tokio::fs::read_to_string(&manifest_path).await {
            serde_json::from_str(&data).ok()
        } else {
            None
        }
    }
}

pub async fn get_or_recover_session(
    session_id: &Uuid,
    music_paths: &[std::path::PathBuf],
    cache_path: &std::path::Path,
) -> Option<ImportSessionState> {
    {
        let sessions = IMPORT_SESSIONS.read().await;
        if let Some(s) = sessions.get(session_id) {
            return Some(s.clone());
        }
    }
    let session_dir = get_session_dir(music_paths, cache_path, session_id);
    if let Some(recovered) = ImportSessionState::load_manifest(&session_dir).await {
        let mut sessions = IMPORT_SESSIONS.write().await;
        sessions.insert(*session_id, recovered.clone());
        return Some(recovered);
    }
    None
}

use lazy_static::lazy_static;

const MAX_IMPORT_SESSIONS: usize = 100;

lazy_static! {
    static ref IMPORT_SESSIONS: Arc<RwLock<HashMap<Uuid, ImportSessionState>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

pub async fn clear_import_sessions_for_test() {
    IMPORT_SESSIONS.write().await.clear();
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

fn get_staging_dir(
    _music_paths: &[std::path::PathBuf],
    cache_path: &std::path::Path,
) -> std::path::PathBuf {
    cache_path.join("import_staging")
}

fn get_session_dir(
    music_paths: &[std::path::PathBuf],
    cache_path: &std::path::Path,
    session_id: &Uuid,
) -> std::path::PathBuf {
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

    let session_dir = get_session_dir(
        &state.config.music_paths,
        &state.config.cache_path,
        &session_id,
    );
    tokio::fs::create_dir_all(&session_dir).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &format!("failed to initialize session staging: {e}"),
        )
    })?;

    let session_state = ImportSessionState {
        session_id,
        total_tracks: body.total_tracks,
        total_playlists: body.total_playlists,
        imported_tracks: 0,
        total_size_bytes: 0,
        device_id,
        seen_hashes: Vec::new(),
        files: Vec::new(),
    };
    session_state.save_manifest(&session_dir).await;

    {
        let mut sessions = IMPORT_SESSIONS.write().await;
        if sessions.len() >= MAX_IMPORT_SESSIONS {
            return Err(v1_error(
                StatusCode::TOO_MANY_REQUESTS,
                "TOO_MANY_SESSIONS",
                "Too many active import sessions. Complete or cancel existing sessions first.",
            ));
        }
        sessions.insert(session_id, session_state);
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

    let session_state = get_or_recover_session(
        &session_id,
        &state.config.music_paths,
        &state.config.cache_path,
    )
    .await
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
            &format!("file exceeds max size of {MAX_FILE_SIZE} bytes"),
        ));
    }
    if session_state.total_size_bytes + data.len() as u64 > MAX_SESSION_SIZE {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "SESSION_SIZE_EXCEEDED",
            &format!("session exceeds max total size of {MAX_SESSION_SIZE} bytes"),
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

    let ext = std::path::Path::new(&body.filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let staging_filename = format!("{data_hash}.{ext}");
    let import_dir = get_session_dir(
        &state.config.music_paths,
        &state.config.cache_path,
        &session_id,
    );
    tokio::fs::create_dir_all(&import_dir).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &e.to_string(),
        )
    })?;

    let file_path = import_dir.join(&staging_filename);
    if !file_path.exists() {
        tokio::fs::write(&file_path, &data).await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "IO_ERROR",
                &e.to_string(),
            )
        })?;
    }

    let mut safe_name = sanitize_filename(&body.filename);
    // Check if session already has a file with the same safe_name but different checksum
    {
        let sessions = IMPORT_SESSIONS.read().await;
        if let Some(s) = sessions.get(&session_id) {
            if s.files
                .iter()
                .any(|f| f.safe_name == safe_name && f.checksum != data_hash)
            {
                let stem = std::path::Path::new(&safe_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("track");
                let short_hash = &data_hash[..8.min(data_hash.len())];
                safe_name = format!("{stem}_{short_hash}.{ext}");
            }
        }
    }

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Uploading, None)
        .await
        .ok();

    let final_dir = state
        .config
        .music_paths
        .first()
        .cloned()
        .unwrap_or_else(|| import_dir.clone());
    let final_path = final_dir.join(&safe_name);

    let remote_track_id = if ALLOWED_AUDIO_EXTS.contains(&ext.as_str()) {
        let tid = michi_core::track_id_from_library_path(&final_dir, &final_path);
        Some(tid)
    } else {
        None
    };

    let import_file_opt = remote_track_id.map(|r_id| ImportFile {
        local_track_id,
        filename: body.filename.clone(),
        safe_name: safe_name.clone(),
        staging_filename: staging_filename.clone(),
        checksum: data_hash.clone(),
        size_bytes: data.len() as u64,
        remote_track_id: r_id,
    });

    let updated_session = {
        let mut sessions = IMPORT_SESSIONS.write().await;
        if let Some(s) = sessions.get_mut(&session_id) {
            s.imported_tracks += 1;
            s.total_size_bytes += data.len() as u64;
            s.seen_hashes.push(data_hash.clone());
            if let Some(imp_file) = import_file_opt {
                s.files.push(imp_file);
            }
            Some(s.clone())
        } else {
            None
        }
    };

    if let Some(s) = updated_session {
        s.save_manifest(&import_dir).await;
    }

    michi_db::update_import_session_progress(&state.db, &session_id, 1, data.len() as u64)
        .await
        .ok();

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
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_state = get_or_recover_session(
        &session_id,
        &state.config.music_paths,
        &state.config.cache_path,
    )
    .await
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

    let staging_dir = get_session_dir(
        &state.config.music_paths,
        &state.config.cache_path,
        &session_id,
    );
    let final_dir = state
        .config
        .music_paths
        .first()
        .cloned()
        .unwrap_or_else(|| staging_dir.clone());

    tokio::fs::create_dir_all(&final_dir).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &format!("failed to ensure destination directory: {e}"),
        )
    })?;

    // Step 1: Pre-validate all destination files before modifying library
    for file in &session_state.files {
        let dest = final_dir.join(&file.safe_name);
        if dest.exists() {
            let existing_data = tokio::fs::read(&dest).await.map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "IO_ERROR",
                    &format!("failed to read existing file {}: {e}", file.safe_name),
                )
            })?;
            let existing_hash = compute_sha256(&existing_data);
            if existing_hash != file.checksum {
                michi_db::set_import_session_status(
                    &state.db,
                    &session_id,
                    &ImportState::Failed,
                    Some("destination conflict: file already exists with different content"),
                )
                .await
                .ok();
                return Err(v1_error(
                    StatusCode::CONFLICT,
                    "DESTINATION_CONFLICT",
                    &format!(
                        "Destination file {} already exists with different checksum. Commit aborted.",
                        file.safe_name
                    ),
                ));
            }
        }
    }

    // Step 2: Perform copy with compensatory rollback tracking
    let mut newly_copied_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut committed_files: Vec<(std::path::PathBuf, ImportFile, String)> = Vec::new();

    for file in &session_state.files {
        let src = staging_dir.join(&file.staging_filename);
        let dest = final_dir.join(&file.safe_name);
        let actual_src = if src.exists() {
            src
        } else {
            staging_dir.join(&file.safe_name)
        };

        if dest.exists() {
            committed_files.push((dest, file.clone(), "already_present".to_string()));
        } else {
            if !actual_src.exists() {
                for p in &newly_copied_paths {
                    let _ = tokio::fs::remove_file(p).await;
                }
                return Err(v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "IO_ERROR",
                    &format!("staging source file for {} not found", file.safe_name),
                ));
            }

            if let Err(e) = tokio::fs::copy(&actual_src, &dest).await {
                for p in &newly_copied_paths {
                    let _ = tokio::fs::remove_file(p).await;
                }
                return Err(v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "IO_ERROR",
                    &format!("failed to copy {} to destination: {e}", file.safe_name),
                ));
            }
            newly_copied_paths.push(dest.clone());
            committed_files.push((dest, file.clone(), "inserted".to_string()));
        }
    }

    // Step 3: Scan imported tracks and attach verified content_hash
    let mut imported_tracks: Vec<michi_core::Track> = Vec::new();
    for (dest, file, _status) in &committed_files {
        if let Some(mut track) = michi_scanner::scan_single_file(&final_dir, dest) {
            track.content_hash = Some(file.checksum.clone());
            imported_tracks.push(track);
        }
    }

    // Step 4: Check for unresolved conflicts
    let mut conflict = false;
    for track in &imported_tracks {
        if let Some(ref hash) = track.content_hash {
            let existing = michi_db::find_tracks_by_content_hash(&state.db, hash)
                .await
                .ok()
                .unwrap_or_default();
            if existing.iter().any(|t| {
                t.id != track.id
                    && t.duration_ms
                        .map(|d| (d as i64 - track.duration_ms.unwrap_or(0) as i64).abs() > 2000)
                        .unwrap_or(false)
            }) {
                conflict = true;
                break;
            }
        }
    }

    if conflict {
        for p in &newly_copied_paths {
            let _ = tokio::fs::remove_file(p).await;
        }
        michi_db::set_import_session_status(
            &state.db,
            &session_id,
            &ImportState::Failed,
            Some("unresolved conflicts"),
        )
        .await
        .ok();
        return Err(v1_error(
            StatusCode::CONFLICT,
            "UNRESOLVED_CONFLICTS",
            "Import has duration conflicts with existing tracks. Rollback and fix metadata before retrying.",
        ));
    }

    // Step 5: Upsert tracks to DB
    if !imported_tracks.is_empty() {
        if let Err(e) = michi_db::upsert_tracks(&state.db, &imported_tracks).await {
            for p in &newly_copied_paths {
                let _ = tokio::fs::remove_file(p).await;
            }
            return Err(v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &format!("failed to upsert imported tracks: {e}"),
            ));
        }
    }

    // Step 6: Build mapping with canonical statuses
    let mut mapping: Vec<serde_json::Value> = Vec::new();
    for (_, file, status) in &committed_files {
        let remote_id = if let Some(tr) = imported_tracks
            .iter()
            .find(|t| t.id == file.remote_track_id)
        {
            tr.id
        } else {
            file.remote_track_id
        };

        mapping.push(serde_json::json!({
            "local_track_id": file.local_track_id.unwrap_or(remote_id),
            "status": status,
            "remote_track_id": remote_id,
            "checksum": file.checksum,
        }));
    }

    // Cleanup staging ONLY after all files copied and DB updated successfully
    cleanup_session_dir(&staging_dir).await;

    michi_db::set_import_session_status(&state.db, &session_id, &ImportState::Committed, None)
        .await
        .ok();
    michi_db::close_import_session(&state.db, &session_id)
        .await
        .ok();

    // Remove from active memory sessions now that commit succeeded
    IMPORT_SESSIONS.write().await.remove(&session_id);

    let _ = state.tx.send(r#"{"type":"library_updated"}"#.to_string());

    Ok(Json(serde_json::json!({
        "tracks_imported": session_state.imported_tracks,
        "playlists_imported": 0,
        "total_size_bytes": session_state.total_size_bytes,
        "mapping": mapping,
    })))
}

pub async fn import_rollback_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Json<serde_json::Value> {
    IMPORT_SESSIONS.write().await.remove(&session_id);
    let staging_dir = get_session_dir(
        &state.config.music_paths,
        &state.config.cache_path,
        &session_id,
    );
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
