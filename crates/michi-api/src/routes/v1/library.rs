use axum::{extract::State, http::StatusCode, Json};

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

pub async fn library_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let stats = michi_db::library_stats(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "tracks": stats.tracks,
        "albums": stats.albums,
        "artists": stats.artists,
    })))
}

pub async fn library_scan_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _ = state.tx.send(r#"{"type":"scan_start"}"#.to_string());

    let music_paths = &state.config.music_paths;
    if music_paths.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "NO_MUSIC_PATHS",
            "no music paths configured",
        ));
    }

    let tracks = michi_scanner::scan_directories(music_paths).await;
    let scanned = tracks.len();
    let saved = michi_db::upsert_tracks(&state.db, &tracks)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let _ = state.tx.send(format!(
        r#"{{"type":"scan_done","scanned":{},"saved":{}}}"#,
        scanned, saved
    ));

    Ok(Json(serde_json::json!({
        "status": "ok",
        "scanned": scanned,
        "saved": saved,
    })))
}
