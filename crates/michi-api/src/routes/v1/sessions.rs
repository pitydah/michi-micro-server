use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};

pub async fn active_streams_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "streams": [] }))
}
