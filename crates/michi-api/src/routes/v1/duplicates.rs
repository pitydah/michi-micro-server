use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};

fn v1_error(s: StatusCode, c: &str, m: &str) -> (StatusCode, Json<serde_json::Value>) {
    (s, Json(serde_json::json!({"error":{"code":c,"message":m}})))
}

pub async fn duplicates_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, Option<i64>, String, i64)>(
        "SELECT t1.id, t1.title, t1.artist, t1.album, t1.duration_ms, t1.format, COUNT(*) as cnt
         FROM tracks t1 JOIN tracks t2 ON t1.title = t2.title AND COALESCE(t1.artist,'') = COALESCE(t2.artist,'') AND t1.id != t2.id
         GROUP BY t1.id ORDER BY t1.title ASC LIMIT 200"
    )
        .fetch_all(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    let duplicates: Vec<serde_json::Value> = rows.into_iter().map(|(id, title, artist, album, dur, fmt, cnt)| {
        serde_json::json!({"id": id, "title": title, "artist": artist, "album": album, "duration_ms": dur, "format": fmt, "duplicates": cnt})
    }).collect();

    Ok(Json(
        serde_json::json!({"duplicates": duplicates, "total": duplicates.len()}),
    ))
}
