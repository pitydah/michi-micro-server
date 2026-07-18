use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

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
pub struct SearchQuery {
    pub q: String,
}

pub async fn search_advanced_handler(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if query.q.trim().is_empty() {
        return Ok(Json(serde_json::json!({ "tracks": [], "query": "" })));
    }

    let tracks = michi_db::search_tracks_advanced(&state.db, query.q.trim())
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SEARCH_ERROR",
                &e.to_string(),
            )
        })?;

    let items: Vec<serde_json::Value> = tracks
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "album_artist": t.album_artist,
                "duration_ms": t.duration_ms,
                "format": t.format,
                "genre": t.genre,
                "year": t.year,
                "artwork_id": t.artwork_id,
                "rating": t.rating,
                "starred": t.starred,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "tracks": items,
        "query": query.q.trim(),
    })))
}
