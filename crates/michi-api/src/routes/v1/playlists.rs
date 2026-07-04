use axum::{
    extract::{Path, State},
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

pub async fn playlists_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let playlists = michi_db::list_playlists(&state.db, None)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "playlists": playlists })))
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaylistBody {
    pub name: String,
    pub description: Option<String>,
}

pub async fn create_playlist_handler(
    State(state): State<AppState>,
    Json(body): Json<CreatePlaylistBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "playlist name is required",
        ));
    }
    let input = michi_core::PlaylistCreate {
        name: body.name.trim().to_string(),
        description: body.description,
    };
    let playlist = michi_db::create_playlist(&state.db, &input, None)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
    Ok(Json(serde_json::json!({ "playlist": playlist })))
}

pub async fn get_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let playlist = michi_db::get_playlist(&state.db, &id)
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
                "NOT_FOUND",
                &format!("playlist not found: {}", id),
            )
        })?;
    Ok(Json(serde_json::json!({ "playlist": playlist })))
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlaylistBody {
    pub name: Option<String>,
    pub description: Option<String>,
}

pub async fn update_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(_body): Json<UpdatePlaylistBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_db::get_playlist(&state.db, &id)
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
                "NOT_FOUND",
                &format!("playlist not found: {}", id),
            )
        })?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

pub async fn delete_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_playlist(&state.db, &id)
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
            &format!("playlist not found: {}", id),
        ));
    }
    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

// ── Smart Playlists ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SmartPlaylistBody {
    pub name: String,
    pub rule: String,
    pub params: Option<serde_json::Value>,
}

pub async fn smart_playlist_handler(
    State(state): State<AppState>,
    Json(body): Json<SmartPlaylistBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "playlist name is required",
        ));
    }

    let limit = body.params.as_ref()
        .and_then(|p| p.get("limit").and_then(|v| v.as_u64()))
        .unwrap_or(50)
        .min(200) as usize;

    let tracks = match body.rule.as_str() {
        "most_played" => {
            let rows = sqlx::query_as::<_, (String, i64)>(
                "SELECT track_id, COUNT(*) as cnt FROM play_history GROUP BY track_id ORDER BY cnt DESC LIMIT ?"
            )
            .bind(limit as i64)
            .fetch_all(&state.db)
            .await
            .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

            let mut result = Vec::new();
            for (tid_str, _) in rows {
                if let Ok(tid) = Uuid::parse_str(&tid_str) {
                    if let Ok(Some(t)) = michi_db::get_track(&state.db, &tid).await {
                        result.push(t);
                    }
                }
            }
            result
        }
        "favorites" => {
            michi_db::get_starred_tracks(&state.db)
                .await
                .unwrap_or_default()
        }
        "newest" => {
            let mut t = michi_db::list_tracks(&state.db).await.unwrap_or_default();
            t.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            t.truncate(limit);
            t
        }
        "recently_played" => {
            let rows = sqlx::query_as::<_, (String, String)>(
                "SELECT track_id, MAX(played_at) as last FROM play_history GROUP BY track_id ORDER BY last DESC LIMIT ?"
            )
            .bind(limit as i64)
            .fetch_all(&state.db)
            .await
            .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string()))?;

            let mut result = Vec::new();
            for (tid_str, _) in rows {
                if let Ok(tid) = Uuid::parse_str(&tid_str) {
                    if let Ok(Some(t)) = michi_db::get_track(&state.db, &tid).await {
                        result.push(t);
                    }
                }
            }
            result
        }
        "unplayed" => {
            let all = michi_db::list_tracks(&state.db).await.unwrap_or_default();
            let played_ids: std::collections::HashSet<Uuid> = sqlx::query_as::<_, (String,)>(
                "SELECT DISTINCT track_id FROM play_history"
            )
            .fetch_all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id,)| Uuid::parse_str(&id).ok())
            .collect();

            all.into_iter()
                .filter(|t| !played_ids.contains(&t.id))
                .take(limit)
                .collect()
        }
        "by_genre" => {
            let genre = body.params.as_ref()
                .and_then(|p| p.get("genre").and_then(|v| v.as_str()))
                .unwrap_or("");
            let all = michi_db::list_tracks(&state.db).await.unwrap_or_default();
            all.into_iter()
                .filter(|t| t.genre.as_deref() == Some(genre))
                .take(limit)
                .collect()
        }
        "by_year" => {
            let year = body.params.as_ref()
                .and_then(|p| p.get("year").and_then(|v| v.as_i64()))
                .unwrap_or(2020) as i32;
            let all = michi_db::list_tracks(&state.db).await.unwrap_or_default();
            all.into_iter()
                .filter(|t| t.year == Some(year))
                .take(limit)
                .collect()
        }
        "random" => {
            let all = michi_db::list_tracks(&state.db).await.unwrap_or_default();
            use rand::seq::SliceRandom;
            all.choose_multiple(&mut rand::thread_rng(), limit).cloned().collect()
        }
        _ => {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_RULE",
                &format!("unknown rule: {}. Supported: most_played, favorites, newest, recently_played, unplayed, by_genre, by_year, random", body.rule),
            ));
        }
    };

    // Create the playlist
    let input = michi_core::PlaylistCreate {
        name: body.name.trim().to_string(),
        description: Some(format!("Smart playlist: {}", body.rule)),
    };

    let playlist = michi_db::create_playlist(&state.db, &input, None)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    // Add tracks
    for track in &tracks {
        let _ = michi_db::add_track_to_playlist(&state.db, &playlist.id, &track.id).await;
    }

    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());

    Ok(Json(serde_json::json!({
        "playlist": playlist,
        "tracks_added": tracks.len(),
        "rule": body.rule,
    })))
}

