use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListJobsQuery {
    pub kind: Option<String>,
    pub state: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateJobBody {
    pub kind: String,
    pub priority: Option<i32>,
    pub payload: Option<serde_json::Value>,
}

pub async fn list_jobs_handler(
    State(state): State<AppState>,
    Query(query): Query<ListJobsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let limit = query.limit.unwrap_or(50).min(200);
    let offset = query.offset.unwrap_or(0);
    let jobs = michi_db::list_jobs(
        &state.db,
        query.kind.as_deref(),
        query.state.as_deref(),
        limit,
        offset,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": {"code": "DB_ERROR", "message": e.to_string()}})),
        )
    })?;
    Ok(Json(serde_json::json!({ "jobs": jobs, "limit": limit, "offset": offset })))
}

pub async fn create_job_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateJobBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let priority = body.priority.unwrap_or(0);
    let job = michi_db::create_job(&state.db, &body.kind, priority, body.payload.as_ref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"code": "DB_ERROR", "message": e.to_string()}})),
            )
        })?;
    crate::record_audit(&state.db, "job_created", Some("job"), Some(&job.id), None).await;
    Ok(Json(serde_json::json!({ "job": job })))
}

pub async fn get_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let job = michi_db::get_job(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"code": "DB_ERROR", "message": e.to_string()}})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": {"code": "NOT_FOUND", "message": "job not found"}})),
            )
        })?;
    Ok(Json(serde_json::json!({ "job": job })))
}

pub async fn cancel_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_db::cancel_job(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"code": "DB_ERROR", "message": e.to_string()}})),
            )
        })?;
    crate::record_audit(&state.db, "job_cancelled", Some("job"), Some(&id), None).await;
    Ok(Json(serde_json::json!({ "status": "cancelled", "job_id": id })))
}
