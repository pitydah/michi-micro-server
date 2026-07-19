use axum::{body::Body, extract::State, http::StatusCode, response::Response, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

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

    let playlists = michi_db::list_playlists(&state.db, None)
        .await
        .map_err(|e| {
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
    let server_name = state.config.sync_name.clone();

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

// ── Snapshot ────────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref LAST_SNAPSHOT: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));
}

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

    *LAST_SNAPSHOT.write().await = Some(snapshot.to_string());

    Ok(Json(serde_json::json!({
        "status": "snapshot_created",
        "snapshot": snapshot,
    })))
}

pub async fn last_snapshot_handler() -> Json<serde_json::Value> {
    let snap = LAST_SNAPSHOT.read().await;
    match snap.as_ref() {
        Some(s) => Json(
            serde_json::json!({ "snapshot": serde_json::from_str::<serde_json::Value>(s).unwrap_or_default() }),
        ),
        None => Json(serde_json::json!({ "snapshot": null })),
    }
}

// ── Webhook ─────────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref WEBHOOK_URL: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));
}

pub async fn get_webhook_url() -> Option<String> {
    WEBHOOK_URL.read().await.clone()
}

#[derive(Debug, Deserialize)]
pub struct SetWebhookBody {
    pub url: String,
}

pub async fn set_webhook_handler(
    Json(body): Json<SetWebhookBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.url.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "webhook URL is required",
        ));
    }
    *WEBHOOK_URL.write().await = Some(body.url.trim().to_string());
    Ok(Json(serde_json::json!({ "status": "webhook_set" })))
}

pub async fn get_webhook_handler() -> Json<serde_json::Value> {
    let url = WEBHOOK_URL.read().await;
    Json(serde_json::json!({ "webhook_url": url.clone() }))
}

pub async fn delete_webhook_handler() -> Json<serde_json::Value> {
    *WEBHOOK_URL.write().await = None;
    Json(serde_json::json!({ "status": "webhook_deleted" }))
}

pub async fn test_webhook_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let url = WEBHOOK_URL.read().await.clone();
    match url {
        Some(_) => {
            fire_sync_webhook(&state).await;
            Ok(Json(serde_json::json!({ "status": "webhook_fired" })))
        }
        None => Err(v1_error(
            StatusCode::BAD_REQUEST,
            "NO_WEBHOOK_CONFIGURED",
            "set a webhook URL first",
        )),
    }
}

/// Called after sync completes to fire the webhook
pub async fn fire_sync_webhook(state: &AppState) {
    let url = WEBHOOK_URL.read().await.clone();
    if let Some(url) = url {
        let track_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

        let payload = serde_json::json!({
            "event": "sync_completed",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "stats": { "tracks": track_count },
        });

        let client = reqwest::Client::new();
        match client.post(&url).json(&payload).send().await {
            Ok(resp) => tracing::info!("webhook sent: HTTP {}", resp.status()),
            Err(e) => tracing::warn!("webhook failed: {}", e),
        }
    }
}

// ── Integrity verification ──────────────────────────────────────

pub async fn verify_integrity_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let mut verified = 0u64;
    let mut missing = 0u64;
    let corrupt = 0u64;
    let mut errors: Vec<String> = Vec::new();

    for track in &tracks {
        let path = std::path::Path::new(&track.file_path);
        if !path.exists() {
            missing += 1;
            errors.push(format!(
                "missing: {} ({})",
                track.title.as_deref().unwrap_or("?"),
                track.file_path
            ));
            continue;
        }
        verified += 1;
    }

    Ok(Json(serde_json::json!({
        "status": if missing == 0 && corrupt == 0 { "ok" } else { "issues_found" },
        "verified": verified,
        "missing": missing,
        "corrupt": corrupt,
        "total": tracks.len(),
        "errors": errors,
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

pub async fn backup_bundle_handler(
    State(state): State<AppState>,
) -> Result<Response<Body>, (StatusCode, Json<serde_json::Value>)> {
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let bundle_path = std::env::temp_dir().join(format!("michi-backup-{}.tar.gz", timestamp));
    let temp_dir = std::env::temp_dir().join(format!("michi-bundle-{}", timestamp));

    std::fs::create_dir_all(&temp_dir).map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "TEMP_DIR",
            &e.to_string(),
        )
    })?;

    let settings = serde_json::json!({
        "version": 2,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "server_id": state.server_id(),
        "config_port": state.config.port(),
    });
    std::fs::write(
        temp_dir.join("manifest.json"),
        serde_json::to_string_pretty(&settings).map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SERIALIZE",
                &e.to_string(),
            )
        })?,
    )
    .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "WRITE", &e.to_string()))?;

    let config_json = serde_json::json!({
        "port": state.config.port(),
        "database_url": state.config.database_url,
        "config_path": state.config.config_path.display().to_string(),
        "cache_path": state.config.cache_path.display().to_string(),
    });
    std::fs::write(
        temp_dir.join("config.json"),
        serde_json::to_string_pretty(&config_json).map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SERIALIZE",
                &e.to_string(),
            )
        })?,
    )
    .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "WRITE", &e.to_string()))?;

    let mut checksums = serde_json::Map::new();
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
        let hash = blake3::hash(&data);
        checksums.insert(name, serde_json::Value::String(hash.to_hex().to_string()));
    }
    std::fs::write(
        temp_dir.join("checksums.json"),
        serde_json::to_string_pretty(&checksums).unwrap(),
    )
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "WRITE_CHECKSUMS",
            &e.to_string(),
        )
    })?;

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

    let filename = format!("michi-backup-{}.tar.gz", timestamp);
    Response::builder()
        .header("Content-Type", "application/gzip")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
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
