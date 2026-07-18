use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
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

#[derive(Serialize)]
pub struct RecentPlay {
    pub track_id: Uuid,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub played_at: String,
}

#[derive(Serialize)]
pub struct LibraryStats {
    pub tracks: i64,
    pub albums: i64,
    pub artists: i64,
    pub genres: i64,
    pub total_duration_ms: i64,
}

#[derive(Serialize)]
pub struct PlaybackInfo {
    pub has_current: bool,
    pub track_id: Option<Uuid>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub state: String,
    pub position_ms: u64,
    pub volume: f64,
}

#[derive(Serialize)]
pub struct HealthInfo {
    pub missing_files: i64,
    pub tracks_without_genre: i64,
    pub tracks_without_year: i64,
    pub tracks_without_cover: i64,
    pub is_healthy: bool,
}

#[derive(Serialize)]
pub struct EcosystemInfo {
    pub receivers_online: usize,
    pub sync_peers: usize,
    pub webhook_configured: bool,
    pub uploads_in_progress: usize,
}

#[derive(Serialize)]
pub struct DashboardResponse {
    pub library: LibraryStats,
    pub playback: PlaybackInfo,
    pub recent: Vec<RecentPlay>,
    pub health: HealthInfo,
    pub ecosystem: EcosystemInfo,
}

pub async fn dashboard_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tracks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(&state.db).await.unwrap_or(0);

    let albums: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT album) FROM tracks WHERE album IS NOT NULL AND album != ''")
        .fetch_one(&state.db).await.unwrap_or(0);

    let artists: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT artist) FROM tracks WHERE artist IS NOT NULL AND artist != ''")
        .fetch_one(&state.db).await.unwrap_or(0);

    let genres: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT genre) FROM tracks WHERE genre IS NOT NULL AND genre != ''")
        .fetch_one(&state.db).await.unwrap_or(0);

    let total_duration_ms: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(duration_ms), 0) FROM tracks")
        .fetch_one(&state.db).await.unwrap_or(0);

    let missing_files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE file_path IS NULL OR file_path = ''")
        .fetch_one(&state.db).await.unwrap_or(0);

    let without_genre: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE genre IS NULL OR genre = ''")
        .fetch_one(&state.db).await.unwrap_or(0);

    let without_year: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE year IS NULL")
        .fetch_one(&state.db).await.unwrap_or(0);

    let without_cover: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE artwork_id IS NULL")
        .fetch_one(&state.db).await.unwrap_or(0);

    // Recent plays
    let recent_rows: Vec<(String, String, String, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT h.track_id, h.played_at, COALESCE(t.title, 'Unknown'), t.artist, t.album
             FROM play_history h LEFT JOIN tracks t ON h.track_id = t.id
             ORDER BY h.played_at DESC LIMIT 10"
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let recent: Vec<RecentPlay> = recent_rows
        .into_iter()
        .filter_map(|(tid, played_at, title, artist, album)| {
            Uuid::parse_str(&tid).ok().map(|track_id| RecentPlay {
                track_id,
                title,
                artist,
                album,
                played_at,
            })
        })
        .collect();

    // Playback state
    let playback_state = state.playback_state.read().await;
    let playback = PlaybackInfo {
        has_current: playback_state.track_id.is_some(),
        track_id: playback_state.track_id,
        title: None,
        artist: None,
        album: None,
        state: if playback_state.playing { "playing" } else { "paused" }.to_string(),
        position_ms: playback_state.position_ms,
        volume: playback_state.volume,
    };
    drop(playback_state);

    // Ecosystem
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let receivers_online = reg_read.list().iter().filter(|e| e.paired).count();
    drop(reg_read);

    let sync_peers = state.sync_tx.receiver_count();

    let webhook_url_val = crate::routes::v1::backup::get_webhook_url().await;
    let webhook_configured = webhook_url_val.is_some();

    let uploads_in_progress = 0; // simplified

    let library = LibraryStats {
        tracks, albums, artists, genres, total_duration_ms,
    };

    let health = HealthInfo {
        missing_files,
        tracks_without_genre: without_genre,
        tracks_without_year: without_year,
        tracks_without_cover: without_cover,
        is_healthy: missing_files == 0 && without_genre < (tracks / 10).max(1),
    };

    let ecosystem = EcosystemInfo {
        receivers_online,
        sync_peers,
        webhook_configured,
        uploads_in_progress,
    };

    Ok(Json(serde_json::json!({
        "library": library,
        "playback": playback,
        "recent": recent,
        "health": health,
        "ecosystem": ecosystem,
    })))
}
