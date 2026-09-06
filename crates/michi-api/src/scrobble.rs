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
                message: format!("database error: {e}"),
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
            let played_at = now.timestamp();

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
                    message: format!("database error: {e}"),
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

fn listenbrainz_base_url() -> String {
    std::env::var("MICHI_LISTENBRAINZ_BASE_URL")
        .unwrap_or_else(|_| "https://api.listenbrainz.org".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn lastfm_api_base_url() -> String {
    std::env::var("MICHI_LASTFM_API_BASE_URL")
        .unwrap_or_else(|_| "https://ws.audioscrobbler.com/2.0/".to_string())
}

/// Validate ListenBrainz token against GET /1/validate-token
pub async fn validate_listenbrainz_token(token: &str) -> Result<bool, String> {
    let base = listenbrainz_base_url();
    let url = format!("{base}/1/validate-token");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Token {token}"))
        .send()
        .await
        .map_err(|e| format!("ListenBrainz request failed: {e}"))?;

    if !resp.status().is_success() {
        return Ok(false);
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse ListenBrainz response: {e}"))?;

    // ListenBrainz returns { "code": 200, "message": "Token valid.", "valid": true, "user_name": "..." }
    if let Some(valid) = json.get("valid").and_then(|v| v.as_bool()) {
        Ok(valid)
    } else {
        Ok(json.get("code").and_then(|c| c.as_i64()) == Some(200))
    }
}

/// Calculate Last.fm MD5 api_sig: md5(sorted(params) + secret)
pub fn calculate_lastfm_signature(params: &[(&str, &str)], secret: &str) -> String {
    let mut sorted_params = params.to_vec();
    sorted_params.sort_by(|a, b| a.0.cmp(b.0));

    let mut sig_base = String::new();
    for (key, val) in sorted_params {
        if key != "format" && key != "api_sig" {
            sig_base.push_str(key);
            sig_base.push_str(val);
        }
    }
    sig_base.push_str(secret);

    let digest = md5::compute(sig_base.as_bytes());
    format!("{:x}", digest)
}

async fn submit_listenbrainz(
    db: &sqlx::SqlitePool,
    token: &str,
    play_id: &Uuid,
    track_id: &Uuid,
    listened_at: u64,
) {
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

    let base = listenbrainz_base_url();
    let url = format!("{base}/1/submit-listens");
    let client = reqwest::Client::new();
    match client
        .post(&url)
        .header("Authorization", format!("Token {token}"))
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

async fn submit_lastfm(db: &sqlx::SqlitePool, token: &str, track_id: &Uuid, listened_at: i64) {
    let track = match michi_db::get_track(db, track_id).await {
        Ok(Some(t)) => t,
        _ => return,
    };

    let artist = track.artist.unwrap_or_else(|| "Unknown Artist".to_string());
    let title = track.title.unwrap_or_else(|| "Unknown Track".to_string());
    let album = track.album.unwrap_or_default();

    let api_key = std::env::var("MICHI_LASTFM_API_KEY")
        .unwrap_or_else(|_| "michi_lastfm_proxy".to_string());
    let shared_secret = std::env::var("MICHI_LASTFM_SHARED_SECRET").unwrap_or_default();

    let listened_at_str = listened_at.to_string();
    let mut params_vec: Vec<(&str, &str)> = vec![
        ("method", "track.scrobble"),
        ("api_key", &api_key),
        ("sk", token),
        ("artist", &artist),
        ("track", &title),
        ("album", &album),
        ("timestamp", &listened_at_str),
    ];

    let api_sig;
    if !shared_secret.is_empty() {
        api_sig = calculate_lastfm_signature(&params_vec, &shared_secret);
        params_vec.push(("api_sig", &api_sig));
    }
    params_vec.push(("format", "json"));

    let client = reqwest::Client::new();
    let url = lastfm_api_base_url();
    match client
        .post(&url)
        .form(&params_vec)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                let body_str = resp.text().await.unwrap_or_default();
                if body_str.contains("\"error\"") {
                    tracing::warn!("last.fm scrobble response contained error payload: {}", body_str);
                } else {
                    tracing::info!("last.fm scrobble submitted for track {}", track_id);
                }
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

// ── Integration Management Endpoints ──────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
pub struct ScrobbleStatusResponse {
    pub scrobble_enabled: bool,
    pub listenbrainz_configured: bool,
    pub lastfm_configured: bool,
}

pub async fn get_scrobble_status_handler(
    State(state): State<AppState>,
) -> Json<ScrobbleStatusResponse> {
    let disk_cfg = state.config.read_file_config();
    let scrobble_enabled = disk_cfg
        .as_ref()
        .map(|d| d.scrobble_enabled)
        .unwrap_or(state.config.scrobble_enabled);
    let listenbrainz_configured = disk_cfg
        .as_ref()
        .and_then(|d| d.listenbrainz_token.as_ref())
        .or(state.config.listenbrainz_token.as_ref())
        .is_some();
    let lastfm_configured = disk_cfg
        .as_ref()
        .and_then(|d| d.lastfm_token.as_ref())
        .or(state.config.lastfm_token.as_ref())
        .is_some();

    Json(ScrobbleStatusResponse {
        scrobble_enabled,
        listenbrainz_configured,
        lastfm_configured,
    })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ListenBrainzConfigRequest {
    pub token: String,
}

pub async fn set_listenbrainz_handler(
    State(state): State<AppState>,
    Json(input): Json<ListenBrainzConfigRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let trimmed = input.token.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": { "code": "VALIDATION_ERROR", "message": "Token cannot be empty" }
            })),
        ));
    }

    // Validate token against ListenBrainz API
    let is_valid = validate_listenbrainz_token(trimmed).await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": { "code": "UPSTREAM_ERROR", "message": e }
            })),
        )
    })?;

    if !is_valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": { "code": "INVALID_TOKEN", "message": "ListenBrainz token validation rejected by upstream service" }
            })),
        ));
    }

    // Persist token in config.json
    let mut cfg = state.config.read_file_config().unwrap_or_else(|| state.config.clone());
    cfg.listenbrainz_token = Some(trimmed.to_string());
    cfg.save_to_file().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": { "code": "SAVE_ERROR", "message": e }
            })),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "configured",
        "provider": "listenbrainz",
        "valid": true,
    })))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LastFmConfigRequest {
    pub token: String,
}

pub async fn set_lastfm_handler(
    State(state): State<AppState>,
    Json(input): Json<LastFmConfigRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let trimmed = input.token.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": { "code": "VALIDATION_ERROR", "message": "Session token cannot be empty" }
            })),
        ));
    }

    // Persist token in config.json
    let mut cfg = state.config.read_file_config().unwrap_or_else(|| state.config.clone());
    cfg.lastfm_token = Some(trimmed.to_string());
    cfg.save_to_file().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": { "code": "SAVE_ERROR", "message": e }
            })),
        )
    })?;

    Ok(Json(serde_json::json!({
        "status": "configured",
        "provider": "lastfm",
    })))
}

pub fn scrobble_router() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/api/v1/integrations/scrobbling", get(get_scrobble_status_handler))
        .route("/api/v1/integrations/listenbrainz", post(set_listenbrainz_handler))
        .route("/api/v1/integrations/lastfm", post(set_lastfm_handler))
}
