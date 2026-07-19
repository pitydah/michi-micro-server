use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

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
    Ok(Json(
        serde_json::json!({ "jobs": jobs, "limit": limit, "offset": offset }),
    ))
}

pub async fn create_job_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateJobBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    const ALLOWED_KINDS: &[&str] = &["scan", "sync", "backup", "cleanup"];
    if !ALLOWED_KINDS.contains(&body.kind.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": "INVALID_JOB_KIND",
                    "message": "job kind must be scan, sync, backup, or cleanup"
                }
            })),
        ));
    }
    if body
        .payload
        .as_ref()
        .and_then(|payload| serde_json::to_vec(payload).ok())
        .map(|payload| payload.len() > 4096)
        .unwrap_or(false)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "error": {"code": "PAYLOAD_TOO_LARGE", "message": "job payload exceeds 4096 bytes"}
            })),
        ));
    }
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
                Json(
                    serde_json::json!({"error": {"code": "NOT_FOUND", "message": "job not found"}}),
                ),
            )
        })?;
    Ok(Json(serde_json::json!({ "job": job })))
}

pub async fn cancel_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(token) = state.job_cancel_tokens.read().await.get(&id).cloned() {
        token.cancel();
    }
    let cancelled = michi_db::cancel_job(&state.db, &id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": {"code": "DB_ERROR", "message": e.to_string()}})),
        )
    })?;
    if !cancelled {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": {"code": "NOT_CANCELLABLE", "message": "job is missing or already terminal"}
            })),
        ));
    }
    crate::record_audit(&state.db, "job_cancelled", Some("job"), Some(&id), None).await;
    Ok(Json(
        serde_json::json!({ "status": "cancelled", "job_id": id }),
    ))
}
