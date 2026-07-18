use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

fn v1_error(s: StatusCode, c: &str, m: &str) -> (StatusCode, Json<serde_json::Value>) {
    (s, Json(serde_json::json!({"error":{"code":c,"message":m}})))
}

#[derive(Deserialize)]
pub struct AnnounceBody {
    pub url: String,
    pub mode: Option<String>,
    pub volume: Option<f64>,
}

pub async fn announce_handler(
    State(state): State<AppState>,
    Json(body): Json<AnnounceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.url.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION",
            "url is required",
        ));
    }
    if !body.url.starts_with("http://") && !body.url.starts_with("https://") {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION",
            "url must be http/https",
        ));
    }

    let mode = body.mode.unwrap_or_else(|| "pause".into());
    let saved = {
        let ps = state.playback_state.read().await;
        serde_json::json!({
            "track_id": ps.track_id,
            "position_ms": ps.position_ms,
            "playing": ps.playing,
            "volume": ps.volume,
        })
    };

    // Pause current playback
    {
        let mut ps = state.playback_state.write().await;
        if mode == "pause" {
            ps.playing = false;
        }
        if let Some(v) = body.volume {
            ps.volume = v;
        }
    }

    Ok(Json(serde_json::json!({
        "status": "announcement_scheduled",
        "mode": mode,
        "url": body.url,
        "saved_state": saved,
    })))
}
