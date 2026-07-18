#![allow(clippy::type_complexity)]
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
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

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct HistoryEntry {
    pub track_id: Uuid,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub played_at: String,
}

pub async fn history_handler(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let limit = query.limit.unwrap_or(50).min(200);
    let offset = query.offset.unwrap_or(0);

    let rows: Vec<(String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT h.track_id, h.played_at, COALESCE(t.title, 'Unknown'), t.artist, t.album
             FROM play_history h LEFT JOIN tracks t ON h.track_id = t.id
             ORDER BY h.played_at DESC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM play_history")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let entries: Vec<HistoryEntry> = rows
        .into_iter()
        .filter_map(|(tid, played_at, title, artist, album)| {
            Uuid::parse_str(&tid).ok().map(|track_id| HistoryEntry {
                track_id,
                title,
                artist,
                album,
                played_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "history": entries,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

pub async fn history_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM play_history")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let unique_tracks: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT track_id) FROM play_history")
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

    let today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM play_history WHERE played_at >= datetime('now', '-1 day')",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let this_week: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM play_history WHERE played_at >= datetime('now', '-7 days')",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let this_month: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM play_history WHERE played_at >= datetime('now', '-30 days')",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total": total,
        "unique_tracks": unique_tracks,
        "today": today,
        "this_week": this_week,
        "this_month": this_month,
    })))
}

pub async fn clear_history_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query("DELETE FROM play_history")
        .execute(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    Ok(Json(serde_json::json!({ "status": "cleared" })))
}

#[derive(Deserialize)]
pub struct ExportQuery {
    pub format: Option<String>,
}

pub async fn history_export_handler(
    State(state): State<AppState>,
    Query(_query): Query<ExportQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows: Vec<(String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT h.track_id, h.played_at, COALESCE(t.title, 'Unknown'), t.artist, t.album
             FROM play_history h LEFT JOIN tracks t ON h.track_id = t.id
             ORDER BY h.played_at DESC LIMIT 10000",
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

    let entries: Vec<serde_json::Value> = rows
        .into_iter()
        .filter_map(|(tid, played_at, title, artist, album)| {
            Uuid::parse_str(&tid).ok().map(|track_id| {
                serde_json::json!({
                    "track_id": track_id,
                    "title": title,
                    "artist": artist,
                    "album": album,
                    "played_at": played_at,
                })
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "entries": entries,
        "total": entries.len(),
    })))
}
