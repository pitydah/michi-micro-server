use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, SaltString};
use rand::rngs::OsRng;

fn v1_error(s: StatusCode, c: &str, m: &str) -> (StatusCode, Json<serde_json::Value>) {
    (s, Json(serde_json::json!({"error":{"code":c,"message":m}})))
}

#[derive(Deserialize)]
pub struct CreateShareBody {
    pub track_id: Uuid,
    pub password: Option<String>,
    pub expires_in_hours: Option<i64>,
    pub max_plays: Option<i64>,
    pub allow_stream: Option<bool>,
    pub allow_download: Option<bool>,
}

pub async fn create_share_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateShareBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &body.track_id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "track not found"))?;

    let token = Uuid::new_v4();
    let token_hash = format!("{:x}", Sha256::digest(token.to_string().as_bytes()));
    let password_hash = body.password.map(|p| {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(p.as_bytes(), &salt)
            .map(|h| h.to_string())
            .unwrap_or_else(|_| format!("{:x}", Sha256::digest(p.as_bytes())))
    });

    let expires_at = body
        .expires_in_hours
        .map(|h| (chrono::Utc::now() + chrono::Duration::hours(h)).to_rfc3339());

    sqlx::query(
        "INSERT INTO shared_links (id, token_hash, track_id, password_hash, expires_at, max_plays, max_downloads, allow_stream, allow_download, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)"
    )
        .bind(Uuid::new_v4().to_string())
        .bind(&token_hash)
        .bind(body.track_id.to_string())
        .bind(&password_hash)
        .bind(&expires_at)
        .bind(body.max_plays)
        .bind(body.allow_stream.unwrap_or(true))
        .bind(body.allow_download.unwrap_or(false))
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "created",
        "share_token": token.to_string(),
        "track_id": body.track_id,
        "track_title": track.title,
        "expires_at": expires_at,
    })))
}

pub async fn list_shares_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<String>, String, Option<i64>, bool, bool, i64, i64)>(
        "SELECT s.id, s.token_hash, s.track_id, s.expires_at, COALESCE(t.title, 'Unknown'), s.max_plays, s.allow_stream, s.allow_download, s.play_count, s.download_count
         FROM shared_links s LEFT JOIN tracks t ON s.track_id = t.id
         WHERE s.expires_at IS NULL OR s.expires_at > datetime('now')
         ORDER BY s.created_at DESC LIMIT 100"
    )
        .fetch_all(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    let shares: Vec<serde_json::Value> = rows.into_iter().map(|(id, _hash, tid, expires, title, max_plays, stream, dl, plays, dls)| {
        serde_json::json!({"id": id, "track_id": tid, "title": title, "expires_at": expires, "max_plays": max_plays, "allow_stream": stream, "allow_download": dl, "play_count": plays, "download_count": dls})
    }).collect();

    Ok(Json(serde_json::json!({"shares": shares})))
}

pub async fn delete_share_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query("DELETE FROM shared_links WHERE id = ?")
        .bind(id.to_string())
        .execute(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({"status": "deleted"})))
}
