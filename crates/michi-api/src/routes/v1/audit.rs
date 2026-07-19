use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use sqlx::SqlitePool;

pub async fn record_audit(
    db: &SqlitePool,
    action: &str,
    entity_type: Option<&str>,
    entity_id: Option<&str>,
    details: Option<serde_json::Value>,
) {
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();
    let details_str = details.map(|d| d.to_string());
    let _ = sqlx::query(
        "INSERT INTO audit_log (id, action, entity_type, entity_id, details_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(&details_str)
    .bind(&now)
    .execute(db)
    .await;

    let _ = sqlx::query(
        "DELETE FROM audit_log WHERE id NOT IN (SELECT id FROM audit_log ORDER BY created_at DESC LIMIT 5000)"
    )
        .execute(db)
        .await;
}

pub async fn audit_log_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        "SELECT action, entity_type, entity_id, details_json, ip_prefix, created_at
         FROM audit_log ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        let s = StatusCode::INTERNAL_SERVER_ERROR;
        (
            s,
            Json(serde_json::json!({"error": {"code": "DB_ERROR", "message": e.to_string()}})),
        )
    })?;

    let entries: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(action, et, eid, details, _ip, created)| {
            serde_json::json!({
                "action": action,
                "entity_type": et,
                "entity_id": eid,
                "details": details.and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok()),
                "created_at": created,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "entries": entries })))
}
