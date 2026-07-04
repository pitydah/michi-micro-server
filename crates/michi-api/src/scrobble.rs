use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct RecordPlayRequest {
    pub track_id: Uuid,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecordPlayResponse {
    pub status: String,
    pub id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlayHistoryEntry {
    pub id: Uuid,
    pub track_id: Uuid,
    pub played_at: String,
    pub duration_ms: Option<u64>,
    pub scrobbled: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_duration_ms: Option<u64>,
    pub artwork_id: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/playback/record",
    tag = "Scrobbling",
    request_body = RecordPlayRequest,
    responses(
        (status = 200, description = "Play recorded", body = RecordPlayResponse),
        (status = 500, description = "Internal server error", body = crate::library::ErrorResponse)
    )
)]
pub async fn record_play_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RecordPlayRequest>,
) -> Result<Json<RecordPlayResponse>, (StatusCode, Json<crate::library::ErrorResponse>)> {
    let now = Utc::now();
    let user_id = state.get_user_id(&headers).await;

    let play = michi_db::record_play(
        &state.db,
        &input.track_id,
        input.duration_ms,
        &now,
        user_id.as_ref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::library::ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    // If ListenBrainz is configured, submit scrobble asynchronously
    if state.config.scrobble_enabled {
        if let Some(token) = &state.config.listenbrainz_token {
            let db = state.db.clone();
            let token = token.clone();
            let play_id = play.id;
            let track_id = input.track_id;
            let played_at = now.timestamp() as u64;

            tokio::spawn(async move {
                submit_listenbrainz(&db, &token, &play_id, &track_id, played_at).await;
            });
        }

        // Also submit to Last.fm if configured
        if let Some(token) = &state.config.lastfm_token {
            let db = state.db.clone();
            let token = token.clone();
            let track_id = input.track_id;
            let played_at = now.timestamp() as i64;

            tokio::spawn(async move {
                submit_lastfm(&db, &token, &track_id, played_at).await;
            });
        }
    }

    Ok(Json(RecordPlayResponse {
        status: "ok".to_string(),
        id: play.id,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/history",
    tag = "Scrobbling",
    params(
        ("limit" = Option<i64>, Query, description = "Maximum number of entries"),
        ("offset" = Option<i64>, Query, description = "Number of entries to skip"),
    ),
    responses(
        (status = 200, description = "Play history", body = Vec<PlayHistoryEntry>),
        (status = 500, description = "Internal server error", body = crate::library::ErrorResponse)
    )
)]
pub async fn history_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<PlayHistoryEntry>>, (StatusCode, Json<crate::library::ErrorResponse>)> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let user_id = state.get_user_id(&headers).await;

    let entries = michi_db::get_play_history(&state.db, limit, offset, user_id.as_ref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(crate::library::ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    let result: Vec<PlayHistoryEntry> = entries
        .into_iter()
        .map(|(ph, t)| PlayHistoryEntry {
            id: ph.id,
            track_id: ph.track_id,
            played_at: ph.played_at.to_rfc3339(),
            duration_ms: ph.duration_ms,
            scrobbled: ph.scrobbled,
            title: t.title,
            artist: t.artist,
            album: t.album,
            album_artist: t.album_artist,
            track_duration_ms: t.duration_ms,
            artwork_id: t.artwork_id,
        })
        .collect();

    Ok(Json(result))
}

async fn submit_listenbrainz(
    db: &sqlx::SqlitePool,
    token: &str,
    play_id: &Uuid,
    track_id: &Uuid,
    listened_at: u64,
) {
    // Look up track metadata
    let track = match michi_db::get_track(db, track_id).await {
        Ok(Some(t)) => t,
        _ => return,
    };

    let artist_name = track.artist.unwrap_or_else(|| "Unknown Artist".to_string());
    let track_name = track.title.unwrap_or_else(|| "Unknown Track".to_string());
    let release_name = track.album;

    let payload = serde_json::json!({
        "listen_type": "single",
        "payload": [{
            "track_metadata": {
                "artist_name": artist_name,
                "track_name": track_name,
                "release_name": release_name,
            },
            "listened_at": listened_at,
        }]
    });

    let client = reqwest::Client::new();
    match client
        .post("https://api.listenbrainz.org/1/submit-listens")
        .header("Authorization", format!("Token {}", token))
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                tracing::info!("scrobble submitted for track {}", track_id);
                let _ = michi_db::mark_scrobbled(db, play_id).await;
            } else {
                tracing::warn!(
                    "scrobble submission failed: {} {}",
                    resp.status(),
                    resp.text().await.unwrap_or_default()
                );
            }
        }
        Err(e) => {
            tracing::warn!("scrobble request error: {}", e);
        }
    }
}

async fn submit_lastfm(
    db: &sqlx::SqlitePool,
    token: &str,
    track_id: &Uuid,
    listened_at: i64,
) {
    let track = match michi_db::get_track(db, track_id).await {
        Ok(Some(t)) => t,
        _ => return,
    };

    let artist = track.artist.unwrap_or_else(|| "Unknown Artist".to_string());
    let title = track.title.unwrap_or_else(|| "Unknown Track".to_string());
    let album = track.album.unwrap_or_default();

    // Last.fm API: POST https://ws.audioscrobbler.com/2.0/
    // Uses token-based auth (Mobile Last.fm Web Auth or API key + shared secret)
    let client = reqwest::Client::new();
    let params = [
        ("method", "track.scrobble"),
        ("api_key", "michi_lastfm_proxy"),
        ("sk", token),
        ("artist", &artist),
        ("track", &title),
        ("album", &album),
        ("timestamp", &listened_at.to_string()),
        ("format", "json"),
    ];

    match client
        .post("https://ws.audioscrobbler.com/2.0/")
        .form(&params)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                tracing::info!("last.fm scrobble submitted for track {}", track_id);
            } else {
                tracing::warn!(
                    "last.fm scrobble failed: {} {}",
                    resp.status(),
                    resp.text().await.unwrap_or_default()
                );
            }
        }
        Err(e) => {
            tracing::warn!("last.fm request error: {}", e);
        }
    }
}
