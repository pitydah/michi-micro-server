use crate::AppState;
use axum::{extract::State, Json};

pub async fn active_streams_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut streams = Vec::new();

    if !state.disabled_modules.read().await.contains("playback") {
        if let Ok(snap) = state.playback_engine.snapshot().await {
            if snap.is_playing() {
                let title = snap.current_track.as_ref().and_then(|t| t.title.clone());
                let artist = snap.current_track.as_ref().and_then(|t| t.artist.clone());
                let album = snap.current_track.as_ref().and_then(|t| t.album.clone());

                streams.push(serde_json::json!({
                    "stream_id": snap.track_id.map(|id| id.to_string()).unwrap_or_else(|| "active-stream".to_string()),
                    "track_id": snap.track_id,
                    "title": title,
                    "artist": artist,
                    "album": album,
                    "position_ms": snap.position_ms,
                    "duration_ms": snap.duration_ms,
                    "volume": snap.volume,
                    "source": "engine",
                    "active": true
                }));
            }
        }
    }

    Json(serde_json::json!({ "streams": streams }))
}
