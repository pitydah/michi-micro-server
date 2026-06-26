use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tracing::info;

use crate::AppState;

pub async fn sync_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_sync(socket, state))
}

async fn handle_sync(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Send identify message
    let identify = michi_sync::SyncMessage::Identify {
        name: state.config.sync_name.clone(),
        version: "0.1.0".into(),
    };
    if let Ok(json) = identify.serialize() {
        let _ = sender.send(Message::Text(json)).await;
    }

    // Subscribe to sync_tx for local state changes
    let mut sync_rx = state.sync_tx.subscribe();

    // Send current state on connect
    {
        let current = state.playback_state.read().await;
        let msg: michi_sync::SyncMessage = current.clone().into();
        if let Ok(json) = msg.serialize() {
            let _ = sender.send(Message::Text(json)).await;
        }
    }

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = sync_rx.recv().await {
            if let Ok(json) = msg.serialize() {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    let state_clone = state.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(sync_msg) = michi_sync::SyncMessage::deserialize(&text) {
                        match &sync_msg {
                            michi_sync::SyncMessage::State {
                                track_id,
                                position_ms,
                                playing,
                                volume,
                                updated_at,
                            } => {
                                info!(
                                    "sync: received state track={:?} pos={} playing={}",
                                    track_id, position_ms, playing
                                );
                                // Update local state
                                let new_state = michi_sync::PlaybackState {
                                    track_id: *track_id,
                                    position_ms: *position_ms,
                                    playing: *playing,
                                    volume: *volume,
                                    updated_at: *updated_at,
                                };
                                {
                                    let mut current = state_clone.playback_state.write().await;
                                    *current = new_state;
                                }
                                // Notify local UI clients
                                let tid = track_id
                                    .map(|id| format!("\"{}\"", id))
                                    .unwrap_or_else(|| "null".into());
                                let payload = format!(
                                    r#"{{"type":"sync_state","track_id":{tid},"position_ms":{position_ms},"playing":{playing},"volume":{volume}}}"#,
                                );
                                let _ = state_clone.tx.send(payload);
                            }
                            michi_sync::SyncMessage::Identify { name, .. } => {
                                info!("sync: peer identified as '{}'", name);
                            }
                            michi_sync::SyncMessage::Ping => {
                                // Pong response would need the sender handle.
                                // Peer will detect liveness via TCP keepalive.
                            }
                            michi_sync::SyncMessage::Pong => {}
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
