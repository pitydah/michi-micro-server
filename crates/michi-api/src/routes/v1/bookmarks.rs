use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
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
pub struct BookmarkBody {
    pub track_id: Uuid,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub finished: Option<bool>,
    pub device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BookmarkQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn upsert_bookmark_handler(
    State(state): State<AppState>,
    Json(body): Json<BookmarkBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_db::upsert_bookmark(
        &state.db,
        &body.track_id,
        "default",
        body.device_id.as_deref(),
        body.position_ms,
        body.duration_ms,
        body.finished.unwrap_or(false),
    )
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({ "status": "saved" })))
}

pub async fn get_bookmark_handler(
    State(state): State<AppState>,
    Path(track_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let bookmark = michi_db::get_bookmark(&state.db, &track_id, "default")
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    match bookmark {
        Some(b) => Ok(Json(serde_json::json!({
            "bookmark": {
                "track_id": b.track_id,
                "position_ms": b.position_ms,
                "duration_ms": b.duration_ms,
                "finished": b.finished,
                "device_id": b.device_id,
            }
        }))),
        None => Ok(Json(serde_json::json!({ "bookmark": null }))),
    }
}

pub async fn list_bookmarks_handler(
    State(state): State<AppState>,
    Query(query): Query<BookmarkQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let limit = query.limit.unwrap_or(50).min(200);
    let offset = query.offset.unwrap_or(0);

    let bookmarks = michi_db::list_bookmarks(&state.db, "default", limit, offset)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let items: Vec<serde_json::Value> = bookmarks
        .into_iter()
        .map(|b| {
            serde_json::json!({
                "track_id": b.track_id,
                "position_ms": b.position_ms,
                "duration_ms": b.duration_ms,
                "finished": b.finished,
                "device_id": b.device_id,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "bookmarks": items })))
}

pub async fn delete_bookmark_handler(
    State(state): State<AppState>,
    Path(track_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_bookmark(&state.db, &track_id, "default")
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    if !deleted {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "bookmark not found",
        ));
    }

    Ok(Json(serde_json::json!({ "status": "deleted" })))
}
