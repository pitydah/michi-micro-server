use crate::AppState;
use axum::{extract::State, Json};

pub async fn active_streams_handler(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "streams": [] }))
}
