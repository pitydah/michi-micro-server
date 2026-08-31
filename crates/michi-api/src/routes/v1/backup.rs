use axum::{body::Body, extract::State, http::StatusCode, response::Response, Json};
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

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupPlaylist {
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<michi_core::Track>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupPayload {
    pub version: u32,
    pub exported_at: String,
    pub tracks: Vec<michi_core::Track>,
    pub playlists: Vec<BackupPlaylist>,
    pub starred_tracks: Vec<michi_core::Track>,
    pub play_history: Vec<BackupHistoryEntry>,
    pub server_id: String,
    pub server_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupHistoryEntry {
    pub track_id: String,
    pub played_at: String,
    pub timestamp: String,
}

pub async fn build_backup_payload(
    state: &AppState,
) -> Result<BackupPayload, (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let playlists_raw = michi_db::list_playlists(&state.db, None)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let mut playlists = Vec::with_capacity(playlists_raw.len());
    for pl in &playlists_raw {
        let track_rows = michi_db::get_playlist_tracks(&state.db, &pl.id)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;
        let mut playlist_tracks = Vec::with_capacity(track_rows.len());
        for pt in &track_rows {
            let track = michi_db::get_track(&state.db, &pt.0.track_id)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &format!("failed to fetch track {}: {e}", pt.0.track_id),
                    )
                })?
                .ok_or_else(|| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_CORRUPTION",
                        &format!("playlist references non-existent track {}", pt.0.track_id),
                    )
                })?;
            playlist_tracks.push(track);
        }
        playlists.push(BackupPlaylist {
            name: pl.name.clone(),
            description: pl.description.clone(),
            tracks: playlist_tracks,
        });
    }

    let starred_tracks = michi_db::get_starred_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let play_history_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT track_id, played_at FROM play_history ORDER BY played_at DESC LIMIT 10000",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let play_history: Vec<BackupHistoryEntry> = play_history_rows
        .into_iter()
        .map(|(track_id, played_at)| {
            let timestamp = played_at.clone();
            BackupHistoryEntry {
                track_id,
                played_at,
                timestamp,
            }
        })
        .collect();

    let server_id = state.server_id().to_string();
    let server_name = state.config.sync_name.clone();

    Ok(BackupPayload {
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        tracks,
        playlists,
        starred_tracks,
        play_history,
        server_id,
        server_name,
    })
}

pub async fn backup_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let backup = build_backup_payload(&state).await?;
    Ok(Json(serde_json::to_value(&backup).unwrap()))
}

pub async fn download_backup_handler(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let payload = build_backup_payload(&state).await?;

    let json_bytes = serde_json::to_vec_pretty(&payload).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "BACKUP_CREATION_FAILED",
            &e.to_string(),
        )
    })?;
    let filename = format!(
        "michi-backup-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(json_bytes))
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_ERROR",
                &e.to_string(),
            )
        })
}

#[derive(Debug, Deserialize)]
pub struct RestoreBody {
    pub tracks: Vec<michi_core::Track>,
    pub playlists: Vec<BackupPlaylist>,
    pub starred_tracks: Vec<michi_core::Track>,
    pub play_history: Vec<BackupHistoryEntry>,
    #[serde(default)]
    pub force: bool,
}

pub async fn restore_handler(
    State(state): State<AppState>,
    Json(body): Json<RestoreBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let existing_tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    if !existing_tracks.is_empty() && !body.force {
        return Err(v1_error(
            StatusCode::CONFLICT,
            "RESTORE_REQUIRES_FORCE",
            "tracks already exist; set force=true to overwrite",
        ));
    }

    if body.force
        && !existing_tracks.is_empty()
        && body.tracks.is_empty()
        && body.starred_tracks.is_empty()
    {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "EMPTY_RESTORE",
            "force=true but no tracks or starred_tracks provided to restore",
        ));
    }

    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let mut restored_tracks = 0u64;
    let mut restored_playlists = 0u64;
    let mut restored_starred = 0u64;
    let mut restored_history = 0u64;

    if body.force {
        sqlx::query("DELETE FROM playlist_tracks")
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &format!("failed to clear playlist_tracks for force restore: {e}"),
                )
            })?;
        sqlx::query("DELETE FROM playlists")
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &format!("failed to clear playlists for force restore: {e}"),
                )
            })?;
        sqlx::query("DELETE FROM play_history")
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &format!("failed to clear play_history for force restore: {e}"),
                )
            })?;
        michi_db::delete_all_tracks_tx(&mut tx).await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &format!("failed to clear tracks for force restore: {e}"),
            )
        })?;
    }

    for track in &body.tracks {
        michi_db::upsert_track_tx(&mut tx, track)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &format!("failed to restore track {}: {e}", track.id),
                )
            })?;
        restored_tracks += 1;
    }

    for pl in &body.playlists {
        let playlist = michi_db::create_playlist_tx(
            &mut tx,
            &michi_core::PlaylistCreate {
                name: pl.name.clone(),
                description: pl.description.clone(),
            },
            None,
        )
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &format!("failed to restore playlist {}: {e}", pl.name),
            )
        })?;
        restored_playlists += 1;

        for track in &pl.tracks {
            michi_db::upsert_track_tx(&mut tx, track)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &format!("failed to upsert playlist track {}: {e}", track.id),
                    )
                })?;
            michi_db::add_track_to_playlist_tx(&mut tx, &playlist.id, &track.id)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &format!(
                            "failed to add track {} to playlist {}: {e}",
                            track.id, pl.name
                        ),
                    )
                })?;
        }
    }

    for track in &body.starred_tracks {
        michi_db::upsert_track_tx(&mut tx, track)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &format!("failed to upsert starred track {}: {e}", track.id),
                )
            })?;
        michi_db::star_track_tx(&mut tx, &track.id, true)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &format!("failed to star track {}: {e}", track.id),
                )
            })?;
        restored_starred += 1;
    }

    for entry in &body.play_history {
        let res =
            sqlx::query("INSERT OR IGNORE INTO play_history (track_id, played_at) VALUES (?, ?)")
                .bind(&entry.track_id)
                .bind(&entry.played_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &format!("failed to restore play history entry: {e}"),
                    )
                })?;
        if res.rows_affected() > 0 {
            restored_history += 1;
        }
    }

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &format!("failed to commit restore transaction: {e}"),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "restored",
        "tracks": restored_tracks,
        "playlists": restored_playlists,
        "starred": restored_starred,
        "history": restored_history,
    })))
}

// ── Snapshot ────────────────────────────────────────────────────

pub async fn snapshot_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let track_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let album_count: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT album) FROM tracks WHERE album IS NOT NULL")
            .fetch_one(&state.db)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;

    let artist_count: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT artist) FROM tracks WHERE artist IS NOT NULL")
            .fetch_one(&state.db)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;

    let snapshot = serde_json::json!({
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "stats": {
            "tracks": track_count,
            "albums": album_count,
            "artists": artist_count,
        },
    });

    michi_db::save_snapshot(&state.db, &snapshot.to_string())
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &format!("failed to save snapshot: {e}"),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "snapshot_created",
        "snapshot": snapshot,
    })))
}

pub async fn last_snapshot_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    match michi_db::last_snapshot(&state.db).await {
        Ok(Some(s)) => Json(
            serde_json::json!({ "snapshot": serde_json::from_str::<serde_json::Value>(&s).unwrap_or_default() }),
        ),
        _ => Json(serde_json::json!({ "snapshot": null })),
    }
}

pub async fn run_auto_backup_cycle(state: &AppState) -> Result<String, String> {
    let backup_dir = state.config.config_path.join("backups");
    tokio::fs::create_dir_all(&backup_dir)
        .await
        .map_err(|e| format!("failed to create backup dir: {e}"))?;

    let backup = build_backup_payload(state).await.map_err(|(_, json_val)| {
        json_val["error"]["message"]
            .as_str()
            .unwrap_or("backup failed")
            .to_string()
    })?;

    let backup_json = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%f").to_string();
    let filename = format!("michi_autobackup_{timestamp}.json");
    let tmp_path = backup_dir.join(format!("{filename}.tmp"));
    let final_path = backup_dir.join(&filename);

    tokio::fs::write(&tmp_path, backup_json.as_bytes())
        .await
        .map_err(|e| format!("failed to write temp backup file: {e}"))?;
    tokio::fs::rename(&tmp_path, &final_path)
        .await
        .map_err(|e| format!("failed to commit backup file: {e}"))?;

    // Also update snapshot in DB
    let _ = snapshot_handler(State(state.clone())).await;

    // Apply retention
    let max_keep = state.config.backup_max_keep.max(1) as usize;
    let mut entries = Vec::new();
    if let Ok(mut dir) = tokio::fs::read_dir(&backup_dir).await {
        while let Ok(Some(entry)) = dir.next_entry().await {
            let p = entry.path();
            if p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.starts_with("michi_autobackup_") && s.ends_with(".json"))
                    .unwrap_or(false)
            {
                entries.push(p);
            }
        }
    }
    entries.sort(); // lexical sort by timestamp in filename
    if entries.len() > max_keep {
        let to_remove = entries.len() - max_keep;
        let mut pruned = 0;
        let mut prune_errors = 0;
        for p in entries.iter().take(to_remove) {
            match tokio::fs::remove_file(p).await {
                Ok(_) => pruned += 1,
                Err(e) => {
                    tracing::warn!(
                        "auto_backup: failed to prune old backup {}: {e}",
                        p.display()
                    );
                    prune_errors += 1;
                }
            }
        }
        tracing::info!(
            "auto_backup: retention policy applied (kept: {}, pruned: {}, errors: {})",
            max_keep,
            pruned,
            prune_errors
        );
    }

    Ok(filename)
}

// ── Webhook ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SetWebhookBody {
    pub url: String,
}

pub async fn set_webhook_handler(
    State(state): State<AppState>,
    Json(body): Json<SetWebhookBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.url.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "webhook URL is required",
        ));
    }
    let _ = michi_db::set_server_config(&state.db, "webhook_url", body.url.trim()).await;
    Ok(Json(serde_json::json!({ "status": "webhook_set" })))
}

pub async fn get_webhook_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let url = michi_db::get_server_config(&state.db, "webhook_url")
        .await
        .ok()
        .flatten();
    Json(serde_json::json!({ "webhook_url": url }))
}

pub async fn delete_webhook_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let _ = michi_db::set_server_config(&state.db, "webhook_url", "").await;
    Json(serde_json::json!({ "status": "webhook_deleted" }))
}

pub async fn test_webhook_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let url = michi_db::get_server_config(&state.db, "webhook_url")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    if url.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "NO_WEBHOOK_CONFIGURED",
            "set a webhook URL first",
        ));
    }

    let track_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let payload = serde_json::json!({
        "event": "webhook_test",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "stats": { "tracks": track_count },
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "CLIENT_ERROR",
                &e.to_string(),
            )
        })?;

    let start = std::time::Instant::now();
    match client.post(&url).json(&payload).send().await {
        Ok(resp) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let status = resp.status();
            if status.is_success() {
                Ok(Json(serde_json::json!({
                    "status": "success",
                    "status_code": status.as_u16(),
                    "elapsed_ms": elapsed_ms,
                })))
            } else {
                Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": {
                            "code": "WEBHOOK_REMOTE_ERROR",
                            "message": format!("Webhook target responded with HTTP {}", status.as_u16()),
                            "details": {
                                "status_code": status.as_u16(),
                                "elapsed_ms": elapsed_ms,
                            }
                        }
                    })),
                ))
            }
        }
        Err(e) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            Err((
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": {
                        "code": "WEBHOOK_FAILED",
                        "message": format!("Webhook connection failed: {e}"),
                        "details": {
                            "elapsed_ms": elapsed_ms,
                        }
                    }
                })),
            ))
        }
    }
}

/// Called after sync completes to fire the webhook
pub async fn fire_sync_webhook(state: &AppState) {
    let url = michi_db::get_server_config(&state.db, "webhook_url")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if url.is_empty() {
        return;
    }
    let track_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let payload = serde_json::json!({
        "event": "sync_completed",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "stats": { "tracks": track_count },
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.post(&url).json(&payload).send().await {
        Ok(resp) => tracing::info!("webhook sent: HTTP {}", resp.status()),
        Err(e) => tracing::warn!("webhook failed: {}", e),
    }
}

// ── Integrity & Availability verification ──────────────────────

pub async fn library_availability_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let mut available = 0u64;
    let mut missing = 0u64;
    let mut missing_tracks: Vec<String> = Vec::new();

    for track in &tracks {
        let path = std::path::Path::new(&track.file_path);
        if !path.exists() {
            missing += 1;
            missing_tracks.push(format!(
                "missing: {} ({})",
                track.title.as_deref().unwrap_or("?"),
                track.file_path
            ));
            continue;
        }
        available += 1;
    }

    Ok(Json(serde_json::json!({
        "status": if missing == 0 { "ok" } else { "degraded" },
        "available": available,
        "missing": missing,
        "total": tracks.len(),
        "missing_files": missing_tracks,
    })))
}

pub async fn verify_integrity_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let quick_check: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &format!("PRAGMA quick_check failed: {e}"),
            )
        })?;

    let fk_rows: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &format!("PRAGMA foreign_key_check failed: {e}"),
            )
        })?;

    let is_ok = quick_check == "ok" && fk_rows.is_empty();

    Ok(Json(serde_json::json!({
        "status": if is_ok { "ok" } else { "corrupt" },
        "quick_check": quick_check,
        "foreign_key_violations": fk_rows.len(),
        "integrity_verified": is_ok,
    })))
}

pub async fn mount_health_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let paths = &state.config.music_paths;
    let results = michi_db::check_mount_health(paths).await;
    for (path, st, err) in &results {
        let _ = michi_db::update_mount_state(&state.db, path, st, err).await;
    }
    let states = michi_db::get_mount_states(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;
    let all_online = states.iter().all(|(_, s, _, _, _)| s == "online");
    Ok(Json(serde_json::json!({
        "healthy": all_online,
        "mounts": states.into_iter().map(|(p, s, lc, lo, err)| {
            serde_json::json!({"path": p, "state": s, "last_checked": lc, "last_online": lo, "error": err})
        }).collect::<Vec<_>>(),
    })))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ManifestFileEntry {
    pub path: String,
    pub role: String,
    pub size: u64,
    pub blake3: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ManifestV2 {
    pub version: u32,
    pub exported_at: String,
    pub server_id: String,
    pub config_port: u16,
    pub files: Vec<ManifestFileEntry>,
}

pub async fn backup_bundle_handler(
    State(state): State<AppState>,
) -> Result<Response<Body>, (StatusCode, Json<serde_json::Value>)> {
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let bundle_path = std::env::temp_dir().join(format!("michi-backup-{timestamp}.tar.gz"));
    let temp_dir = std::env::temp_dir().join(format!("michi-bundle-{timestamp}"));

    std::fs::create_dir_all(&temp_dir).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "TEMP_DIR",
            &e.to_string(),
        )
    })?;

    // 1. Write full logical backup.json (P1-10 fail-closed on DB errors)
    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    let playlists_raw = michi_db::list_playlists(&state.db, None)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    let mut playlists = Vec::with_capacity(playlists_raw.len());
    for pl in &playlists_raw {
        let track_rows = michi_db::get_playlist_tracks(&state.db, &pl.id)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;
        let mut pl_tracks = Vec::with_capacity(track_rows.len());
        for pt in &track_rows {
            if let Some(track) = michi_db::get_track(&state.db, &pt.0.track_id)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &e.to_string(),
                    )
                })?
            {
                pl_tracks.push(track);
            }
        }
        playlists.push(BackupPlaylist {
            name: pl.name.clone(),
            description: pl.description.clone(),
            tracks: pl_tracks,
        });
    }
    let starred_tracks = michi_db::get_starred_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    let play_history_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT track_id, played_at FROM play_history ORDER BY played_at DESC LIMIT 10000",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    let play_history: Vec<BackupHistoryEntry> = play_history_rows
        .into_iter()
        .map(|(track_id, played_at)| {
            let timestamp = played_at.clone();
            BackupHistoryEntry {
                track_id,
                played_at,
                timestamp,
            }
        })
        .collect();

    let backup_payload = BackupPayload {
        version: 2,
        exported_at: chrono::Utc::now().to_rfc3339(),
        tracks,
        playlists,
        starred_tracks,
        play_history,
        server_id: state.server_id().to_string(),
        server_name: state.config.sync_name.clone(),
    };
    let backup_bytes = serde_json::to_vec_pretty(&backup_payload).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "BACKUP_CREATION_FAILED",
            &e.to_string(),
        )
    })?;
    let backup_file = temp_dir.join("backup.json");
    std::fs::write(&backup_file, &backup_bytes)
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "WRITE", &e.to_string()))?;

    // 2. Write config.json
    let config_json = serde_json::json!({
        "port": state.config.port(),
        "database_url": state.config.database_url,
        "config_path": state.config.config_path.display().to_string(),
        "cache_path": state.config.cache_path.display().to_string(),
    });
    let config_bytes = serde_json::to_vec_pretty(&config_json).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "BACKUP_CREATION_FAILED",
            &e.to_string(),
        )
    })?;
    let config_file = temp_dir.join("config.json");
    std::fs::write(&config_file, &config_bytes)
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "WRITE", &e.to_string()))?;

    // 3. Write checksums.json
    let mut checksums = serde_json::Map::new();
    checksums.insert(
        "backup.json".to_string(),
        serde_json::Value::String(blake3::hash(&backup_bytes).to_hex().to_string()),
    );
    checksums.insert(
        "config.json".to_string(),
        serde_json::Value::String(blake3::hash(&config_bytes).to_hex().to_string()),
    );
    let checksums_bytes = serde_json::to_vec_pretty(&checksums).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "BACKUP_CREATION_FAILED",
            &e.to_string(),
        )
    })?;
    let checksums_file = temp_dir.join("checksums.json");
    std::fs::write(&checksums_file, &checksums_bytes)
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "WRITE", &e.to_string()))?;

    // 4. Build manifest.json (v2 with file hashes)
    let manifest = ManifestV2 {
        version: 2,
        exported_at: chrono::Utc::now().to_rfc3339(),
        server_id: state.server_id().to_string(),
        config_port: state.config.port(),
        files: vec![
            ManifestFileEntry {
                path: "backup.json".to_string(),
                role: "logical_backup".to_string(),
                size: backup_bytes.len() as u64,
                blake3: blake3::hash(&backup_bytes).to_hex().to_string(),
            },
            ManifestFileEntry {
                path: "config.json".to_string(),
                role: "config".to_string(),
                size: config_bytes.len() as u64,
                blake3: blake3::hash(&config_bytes).to_hex().to_string(),
            },
            ManifestFileEntry {
                path: "checksums.json".to_string(),
                role: "checksums".to_string(),
                size: checksums_bytes.len() as u64,
                blake3: blake3::hash(&checksums_bytes).to_hex().to_string(),
            },
        ],
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "BACKUP_CREATION_FAILED",
            &e.to_string(),
        )
    })?;
    let manifest_file = temp_dir.join("manifest.json");
    std::fs::write(&manifest_file, &manifest_bytes)
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "WRITE", &e.to_string()))?;

    let file = std::fs::File::create(&bundle_path)
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "CREATE", &e.to_string()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::best());
    let mut tar = tar::Builder::new(encoder);

    for entry in std::fs::read_dir(&temp_dir).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "READ_DIR",
            &e.to_string(),
        )
    })? {
        let entry = entry
            .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "ENTRY", &e.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let data = std::fs::read(entry.path())
            .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "READ", &e.to_string()))?;
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, &name, std::io::Cursor::new(&data))
            .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "TAR", &e.to_string()))?;
    }

    let encoder = tar.into_inner().map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "TAR_FINISH",
            &e.to_string(),
        )
    })?;
    encoder.finish().map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "GZ_FINISH",
            &e.to_string(),
        )
    })?;

    let bundle_data = std::fs::read(&bundle_path)
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "READ", &e.to_string()))?;

    let _ = std::fs::remove_file(&bundle_path);
    let _ = std::fs::remove_dir_all(&temp_dir);

    let filename = format!("michi-backup-{timestamp}.tar.gz");
    Response::builder()
        .header("Content-Type", "application/gzip")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(bundle_data))
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE",
                &e.to_string(),
            )
        })
}

pub async fn restore_backup_bundle_handler(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "EMPTY_ARCHIVE",
            "Uploaded bundle is empty",
        ));
    }

    let temp_dir = std::env::temp_dir().join(format!("michi-restore-bundle-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "TEMP_DIR",
            &e.to_string(),
        )
    })?;

    // Safe extraction
    let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(body));
    let mut archive = tar::Archive::new(gz);

    let mut extracted_files = std::collections::HashMap::new();
    let entries = archive.entries().map_err(|e| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        v1_error(
            StatusCode::BAD_REQUEST,
            "CORRUPT_ARCHIVE",
            &format!("Failed to read tar.gz entries: {e}"),
        )
    })?;

    for entry in entries {
        let mut entry = entry.map_err(|e| {
            let _ = std::fs::remove_dir_all(&temp_dir);
            v1_error(
                StatusCode::BAD_REQUEST,
                "CORRUPT_ENTRY",
                &format!("Tar entry read failure: {e}"),
            )
        })?;

        let path = entry.path().map_err(|e| {
            let _ = std::fs::remove_dir_all(&temp_dir);
            v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_PATH",
                &format!("Invalid path in archive: {e}"),
            )
        })?;

        let path_str = path.to_string_lossy().to_string();
        if path_str.contains("..") || path_str.starts_with('/') {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "PATH_TRAVERSAL",
                "Archive contains illegal path traversal components",
            ));
        }

        let mut content = Vec::new();
        use std::io::Read;
        entry.read_to_end(&mut content).map_err(|e| {
            let _ = std::fs::remove_dir_all(&temp_dir);
            v1_error(
                StatusCode::BAD_REQUEST,
                "READ_FAILURE",
                &format!("Failed to read content from {path_str}: {e}"),
            )
        })?;

        extracted_files.insert(path_str, content);
    }

    let manifest_bytes = extracted_files.get("manifest.json").ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        v1_error(
            StatusCode::BAD_REQUEST,
            "MISSING_MANIFEST",
            "Archive does not contain manifest.json",
        )
    })?;

    let manifest: ManifestV2 = serde_json::from_slice(manifest_bytes).map_err(|e| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_MANIFEST",
            &format!("manifest.json schema error: {e}"),
        )
    })?;

    if manifest.version != 2 {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "UNSUPPORTED_VERSION",
            &format!(
                "Bundle manifest version {} is not supported (expected 2)",
                manifest.version
            ),
        ));
    }

    // P1-08 Functional Truth: Validate every declared file exists, matches size, and BLAKE3 hash
    for file_entry in &manifest.files {
        let content = extracted_files.get(&file_entry.path).ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&temp_dir);
            v1_error(
                StatusCode::BAD_REQUEST,
                "MISSING_BUNDLE_FILE",
                &format!(
                    "File '{}' declared in manifest is missing from archive",
                    file_entry.path
                ),
            )
        })?;

        if (content.len() as u64) != file_entry.size {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "SIZE_MISMATCH",
                &format!(
                    "File '{}' size mismatch: declared {}, actual {}",
                    file_entry.path,
                    file_entry.size,
                    content.len()
                ),
            ));
        }

        let actual_hash = blake3::hash(content).to_hex().to_string();
        if actual_hash != file_entry.blake3 {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "INTEGRITY_CHECKSUM_FAILED",
                &format!(
                    "BLAKE3 hash mismatch for '{}': declared {}, actual {}",
                    file_entry.path, file_entry.blake3, actual_hash
                ),
            ));
        }
    }

    let backup_bytes = extracted_files.get("backup.json").ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        v1_error(
            StatusCode::BAD_REQUEST,
            "MISSING_BACKUP_JSON",
            "Archive does not contain logical backup.json",
        )
    })?;

    let restore_payload: RestoreBody = serde_json::from_slice(backup_bytes).map_err(|e| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_BACKUP_JSON",
            &format!("backup.json validation failure: {e}"),
        )
    })?;

    let _ = std::fs::remove_dir_all(&temp_dir);

    // Perform database restore inside transaction
    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    sqlx::query("DELETE FROM playlist_tracks")
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    sqlx::query("DELETE FROM playlists")
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    sqlx::query("DELETE FROM play_history")
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    michi_db::delete_all_tracks_tx(&mut tx).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let mut restored_tracks = 0;
    for track in &restore_payload.tracks {
        michi_db::upsert_track_tx(&mut tx, track)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;
        restored_tracks += 1;
    }

    let mut restored_playlists = 0;
    for pl in &restore_payload.playlists {
        let playlist = michi_db::create_playlist_tx(
            &mut tx,
            &michi_core::PlaylistCreate {
                name: pl.name.clone(),
                description: pl.description.clone(),
            },
            None,
        )
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

        for track in &pl.tracks {
            let _ = michi_db::add_track_to_playlist_tx(&mut tx, &playlist.id, &track.id).await;
        }
        restored_playlists += 1;
    }

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "restored",
        "version": 2,
        "tracks_restored": restored_tracks,
        "playlists_restored": restored_playlists,
    })))
}

/// Spawns a background integrity check every 24h
pub fn spawn_integrity_cron(db: sqlx::SqlitePool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
        loop {
            interval.tick().await;
            tracing::info!("integrity check: starting daily scan");
            let tracks = match michi_db::list_tracks(&db).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("integrity check: db error: {}", e);
                    continue;
                }
            };
            let mut missing = 0u64;
            for track in &tracks {
                if !std::path::Path::new(&track.file_path).exists() {
                    missing += 1;
                    tracing::warn!("integrity: missing file: {}", track.file_path);
                }
            }
            tracing::info!(
                "integrity check: {}/{} files ok, {} missing",
                tracks.len() - missing as usize,
                tracks.len(),
                missing
            );
        }
    });
}
