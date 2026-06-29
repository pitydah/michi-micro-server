use axum::{
    extract::ws::{Message, WebSocket},
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tracing::info;

use crate::AppState;

pub async fn events_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events_socket(socket, state))
}

async fn handle_events_socket(socket: WebSocket, state: AppState) {
    info!("events websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // Send initial server status event
    let init_event = json!({
        "type": "server.status",
        "data": {
            "service": "michi-micro-server",
            "version": state.config.version(),
            "server_id": state.server_id(),
            "uptime_seconds": state.started_at.elapsed().as_secs(),
        }
    });
    let _ = sender.send(Message::Text(init_event.to_string())).await;

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_)) = receiver.next().await {}
    });

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    info!("events websocket disconnected");
}
