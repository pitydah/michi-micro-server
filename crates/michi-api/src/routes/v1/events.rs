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

    let init = json!({
        "type": "server.status",
        "data": {
            "service": "michi-micro-server",
            "version": state.config.version(),
            "server_id": state.server_id(),
        }
    });
    let _ = sender.send(Message::Text(init.to_string())).await;

    let send_task = tokio::spawn(async move {
        let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    if let Ok(msg) = msg {
                        if sender.send(Message::Text(msg)).await.is_err() { break; }
                    }
                }
                _ = keepalive.tick() => {
                    if sender.send(Message::Ping(vec![])).await.is_err() { break; }
                }
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
