use axum::{
    extract::State,
    Json,
};

use crate::AppState;

pub async fn library_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let stats = michi_db::library_stats(&state.db).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
        })))
    })?;

    Ok(Json(serde_json::json!({
        "tracks": stats.tracks,
        "albums": stats.albums,
        "artists": stats.artists,
    })))
}

pub async fn library_scan_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let _ = state.tx.send(r#"{"type":"scan_start"}"#.to_string());

    let music_paths = &state.config.music_paths;
    if music_paths.is_empty() {
        return Err((axum::http::StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": { "code": "NO_MUSIC_PATHS", "message": "no music paths configured" }
        }))));
    }

    let tracks = michi_scanner::scan_directories(music_paths).await;
    let scanned = tracks.len();
    let saved = michi_db::upsert_tracks(&state.db, &tracks).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": { "code": "DATABASE_ERROR", "message": e.to_string() }
        })))
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
