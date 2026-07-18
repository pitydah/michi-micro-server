use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::Deserialize;
use uuid::Uuid;
use crate::AppState;

fn v1_error(s: StatusCode, c: &str, m: &str) -> (StatusCode, Json<serde_json::Value>) {
    (s, Json(serde_json::json!({"error":{"code":c,"message":m}})))
}

#[derive(Deserialize)]
pub struct AddSourceBody {
    pub url: String,
}

pub async fn add_source_handler(
    State(state): State<AppState>,
    Json(body): Json<AddSourceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.url.trim().is_empty() {
        return Err(v1_error(StatusCode::BAD_REQUEST, "VALIDATION", "url is required"));
    }

    let info = michi_ingest::sniff_stream(&body.url)
        .await
        .map_err(|e| v1_error(StatusCode::BAD_REQUEST, "SNIFF_ERROR", &e))?;

    if matches!(info.stream_type, michi_ingest::StreamType::Unknown) {
        return Err(v1_error(StatusCode::BAD_REQUEST, "UNKNOWN_STREAM", "could not determine stream type"));
    }

    let source = michi_core::StreamSource {
        id: Uuid::new_v4(),
        url: info.url,
        stream_type: format!("{:?}", info.stream_type).to_lowercase(),
        name: info.name,
        genre: info.genre,
        description: info.description,
        logo_url: info.logo_url,
        codec: info.codec,
        enabled: true,
    };

    michi_db::add_stream_source(&state.db, &source)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    // If podcast, fetch and store episodes
    if source.stream_type == "podcast" {
        if let Ok(body) = reqwest::get(&source.url).await {
            if let Ok(text) = body.text().await {
                let episodes = michi_ingest::parse_rss_episodes(&text);
                for ep in episodes {
                    let db_ep = michi_core::PodcastEpisodeDb {
                        id: Uuid::new_v4(),
                        source_id: source.id,
                        title: ep.title,
                        audio_url: ep.audio_url,
                        pub_date: Some(ep.pub_date),
                        duration_secs: ep.duration_secs,
                        played: false,
                        position_ms: 0,
                    };
                    let _ = michi_db::upsert_podcast_episode(&state.db, &db_ep).await;
                }
            }
        }
    }

    Ok(Json(serde_json::json!({ "source": source })))
}

pub async fn list_sources_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sources = michi_db::list_stream_sources(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;
    Ok(Json(serde_json::json!({ "sources": sources })))
}

pub async fn delete_source_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_stream_source(&state.db, &id)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;
    if !deleted {
        return Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "source not found"));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn get_episodes_handler(
    State(state): State<AppState>,
    Path(source_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let episodes = michi_db::list_podcast_episodes(&state.db, &source_id)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;
    Ok(Json(serde_json::json!({ "episodes": episodes })))
}

#[derive(Deserialize)]
pub struct UpdateEpisodeBody {
    pub position_ms: Option<u64>,
    pub played: Option<bool>,
}

pub async fn update_episode_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateEpisodeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_db::update_episode_progress(&state.db, &id, body.position_ms.unwrap_or(0), body.played.unwrap_or(false))
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;
    Ok(Json(serde_json::json!({ "status": "updated" })))
}
