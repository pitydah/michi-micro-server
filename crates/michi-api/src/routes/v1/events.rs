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
    // Auth check: require Bearer token in the first message
    // But since axum WebSocketUpgrade can't reject after upgrade,
    // we'll validate on first text message
    ws.on_upgrade(move |socket| handle_events_socket(socket, state))
}

async fn handle_events_socket(socket: WebSocket, state: AppState) {
    info!("events websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // First, require client to send auth token
    let auth_ok = if let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);
                match parsed {
                    Ok(val) => {
                        if let Some(token) = val.get("token").and_then(|t| t.as_str()) {
                            // Validate via token_store (link tokens) or auth_sessions (login tokens)
                            let link_valid = state
                                .token_store
                                .validate(token, michi_link::TokenType::Device)
                                .await
                                .is_ok();
                            let sess_valid = if state.auth_enabled {
                                state.auth_sessions.validate(token).await
                            } else {
                                false
                            };
                            link_valid || sess_valid || !state.auth_enabled
                        } else {
                            !state.auth_enabled
                        }
                    }
                    Err(_) => !state.auth_enabled,
                }
            }
            _ => !state.auth_enabled,
        }
    } else {
        !state.auth_enabled
    };

    if !auth_ok {
        info!("events websocket auth failed");
        let _ = sender
            .send(Message::Text(
                json!({
                    "type": "error",
                    "code": "AUTH_REQUIRED",
                    "message": "send {\"token\":\"...\"} as first message"
                })
                .to_string(),
            ))
            .await;
        return;
    }

    info!("events websocket authenticated");

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

    let recv_task = tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }
    info!("events websocket disconnected");
}
