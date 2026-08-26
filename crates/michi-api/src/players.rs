use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct PlayerInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub state: String,
    pub volume: i32,
    pub muted: bool,
    pub current_track_id: Option<String>,
    pub position_ms: i64,
    pub last_seen: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QueueInfo {
    pub id: String,
    pub name: String,
    pub player_id: Option<String>,
    pub current_index: i32,
    pub repeat_mode: String,
    pub shuffle: bool,
    pub items: Vec<QueueItemInfo>,
}

#[derive(Debug, Serialize)]
pub struct QueueItemInfo {
    pub id: String,
    pub track_id: String,
    pub position: i32,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlayerRequest {
    pub name: String,
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateQueueRequest {
    pub name: String,
    pub track_ids: Vec<String>,
}

pub async fn players_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<PlayerInfo>>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            i64,
            i64,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(
        "SELECT id, name, kind, state, volume, muted, current_track_id, position_ms, last_seen FROM players ORDER BY name ASC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    let players = rows
        .into_iter()
        .map(
            |(id, name, kind, state, volume, muted, current_track_id, position_ms, last_seen)| {
                PlayerInfo {
                    id,
                    name,
                    kind,
                    state,
                    volume: volume as i32,
                    muted: muted != 0,
                    current_track_id,
                    position_ms,
                    last_seen,
                }
            },
        )
        .collect();

    Ok(Json(players))
}

pub async fn create_player_handler(
    State(state): State<AppState>,
    Json(body): Json<CreatePlayerRequest>,
) -> Result<Json<PlayerInfo>, (StatusCode, Json<serde_json::Value>)> {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();
    let kind = body.kind.unwrap_or_else(|| "webui".into());

    sqlx::query(
        "INSERT INTO players (id, name, kind, state, created_at, updated_at) VALUES (?, ?, ?, 'idle', ?, ?)",
    )
    .bind(id.to_string())
    .bind(&body.name)
    .bind(&kind)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(PlayerInfo {
        id: id.to_string(),
        name: body.name,
        kind,
        state: "idle".into(),
        volume: 80,
        muted: false,
        current_track_id: None,
        position_ms: 0,
        last_seen: None,
    }))
}

pub async fn queues_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<QueueInfo>>, (StatusCode, Json<serde_json::Value>)> {
    let queue_rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            i64,
            String,
            i64,
        ),
    >(
        "SELECT id, name, player_id, current_index, repeat_mode, shuffle FROM queues ORDER BY name ASC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    let mut queues = Vec::new();
    for (id, name, player_id, current_index, repeat_mode, shuffle) in queue_rows {
        let items_rows = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT id, track_id, position FROM queue_items WHERE queue_id = ? ORDER BY position ASC",
        )
        .bind(&id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let items = items_rows
            .into_iter()
            .map(|(item_id, track_id, position)| QueueItemInfo {
                id: item_id,
                track_id,
                position: position as i32,
            })
            .collect();

        queues.push(QueueInfo {
            id,
            name,
            player_id,
            current_index: current_index as i32,
            repeat_mode,
            shuffle: shuffle != 0,
            items,
        });
    }

    Ok(Json(queues))
}

pub async fn create_queue_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateQueueRequest>,
) -> Result<Json<QueueInfo>, (StatusCode, Json<serde_json::Value>)> {
    let queue_id = Uuid::new_v4();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query("INSERT INTO queues (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(queue_id.to_string())
        .bind(&body.name)
        .bind(&now)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let mut items = vec![];
    for (i, track_id) in body.track_ids.iter().enumerate() {
        let item_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string())
        .bind(queue_id.to_string())
        .bind(track_id)
        .bind(i as i32)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;
        items.push(QueueItemInfo {
            id: item_id.to_string(),
            track_id: track_id.clone(),
            position: i as i32,
        });
    }

    Ok(Json(QueueInfo {
        id: queue_id.to_string(),
        name: body.name,
        player_id: None,
        current_index: 0,
        repeat_mode: "none".into(),
        shuffle: false,
        items,
    }))
}

pub fn players_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/players",
            get(players_handler).post(create_player_handler),
        )
        .route(
            "/api/v1/queues",
            get(queues_handler).post(create_queue_handler),
        )
}
