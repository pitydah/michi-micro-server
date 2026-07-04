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

pub async fn library_health_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let stats = michi_db::library_stats(&state.db).await.map_err(|e| {
        v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string())
    })?;

    let total_playlists = michi_db::list_playlists(&state.db, None)
        .await
        .unwrap_or_default()
        .len() as i64;

    // Count tracks with missing metadata
    let with_title: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE title IS NOT NULL AND title != ''")
        .fetch_one(&state.db).await.unwrap_or(0);
    let with_artist: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE artist IS NOT NULL AND artist != ''")
        .fetch_one(&state.db).await.unwrap_or(0);
    let with_album: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE album IS NOT NULL AND album != ''")
        .fetch_one(&state.db).await.unwrap_or(0);
    let without_meta: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE (title IS NULL OR title = '') AND (artist IS NULL OR artist = '')"
    ).fetch_one(&state.db).await.unwrap_or(0);
    let with_artwork: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE artwork_id IS NOT NULL")
        .fetch_one(&state.db).await.unwrap_or(0);
    let total_duration_ms: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(duration_ms), 0) FROM tracks")
        .fetch_one(&state.db).await.unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total_tracks": stats.tracks,
        "total_albums": stats.albums,
        "total_artists": stats.artists,
        "tracks_with_title": with_title,
        "tracks_with_artist": with_artist,
        "tracks_with_album": with_album,
        "tracks_without_metadata": without_meta,
        "tracks_with_artwork": with_artwork,
        "total_playlists": total_playlists,
        "total_duration_hours": total_duration_ms as f64 / 3600000.0,
    })))
}
