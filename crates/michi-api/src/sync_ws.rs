use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use michi_playback::TrackResolver;
use tracing::{info, warn};

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
        version: env!("CARGO_PKG_VERSION").into(),
        device_type: michi_sync::DeviceType::Server,
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
                                updated_at: _,
                                playlist_id: _,
                                queue_position: _,
                            } => {
                                info!(
                                    "sync: received state track={:?} pos={} playing={}",
                                    track_id, position_ms, playing
                                );
                                // Dispatch state commands to PlaybackEngine so PlaybackProjectionCoordinator
                                // remains the single authoritative writer of PlaybackState.
                                let mut all_applied = true;
                                if let Some(tid) = track_id {
                                    let resolver = michi_playback::SqliteTrackResolver::new(
                                        state_clone.db.clone(),
                                        state_clone.config.music_paths.clone(),
                                    );
                                    match resolver.get_track(*tid).await {
                                        Ok(track) => {
                                            if let Err(e) = state_clone
                                                .playback_engine
                                                .load_track(track, *position_ms)
                                                .await
                                            {
                                                warn!("sync: failed to load track {tid}: {e}");
                                                all_applied = false;
                                            }
                                        }
                                        Err(e) => {
                                            warn!("sync: track {tid} not found locally: {e}");
                                            all_applied = false;
                                        }
                                    }
                                } else if let Err(e) =
                                    state_clone.playback_engine.seek(*position_ms).await
                                {
                                    warn!("sync: failed to seek to {position_ms}ms: {e}");
                                    all_applied = false;
                                }

                                if all_applied {
                                    let play_res = if *playing {
                                        state_clone.playback_engine.resume().await
                                    } else {
                                        state_clone.playback_engine.pause().await
                                    };
                                    if let Err(e) = play_res {
                                        warn!("sync: failed to transition playback state: {e}");
                                        all_applied = false;
                                    }
                                }

                                if all_applied {
                                    let vol_u8 =
                                        ((*volume * 100.0).round().clamp(0.0, 100.0)) as u8;
                                    if let Err(e) =
                                        state_clone.playback_engine.set_volume(vol_u8).await
                                    {
                                        warn!("sync: failed to set volume {vol_u8}: {e}");
                                        all_applied = false;
                                    }
                                }

                                if all_applied {
                                    // Notify local UI clients only when every single operation succeeded
                                    let tid = track_id
                                        .map(|id| format!("\"{id}\""))
                                        .unwrap_or_else(|| "null".into());
                                    let msg = format!(
                                        "{{\"type\":\"sync_state\",\
                                         \"track_id\":{tid},\
                                         \"position_ms\":{position_ms},\
                                         \"playing\":{playing},\
                                         \"volume\":{volume}}}",
                                    );
                                    let _ = state_clone.tx.send(msg);
                                }
                            }
                            michi_sync::SyncMessage::Identify { name, .. } => {
                                info!("sync: peer identified as '{}'", name);
                            }
                            michi_sync::SyncMessage::Ping => {
                                // Pong response would need the sender handle.
                                // Peer will detect liveness via TCP keepalive.
                            }
                            michi_sync::SyncMessage::Pong => {}
                            michi_sync::SyncMessage::HandoffRequest {
                                from_device,
                                to_device,
                            } => {
                                info!(
                                    "sync: handoff request from {} to {}",
                                    from_device, to_device
                                );
                                match state_clone
                                    .sync_manager
                                    .initiate_handoff(from_device.clone(), to_device.clone())
                                    .await
                                {
                                    Ok(session) => {
                                        let mut takeover_ok = true;
                                        if let Some(tid) = session.track_id {
                                            let resolver = michi_playback::SqliteTrackResolver::new(
                                                state_clone.db.clone(),
                                                state_clone.config.music_paths.clone(),
                                            );
                                            match resolver.get_track(tid).await {
                                                Ok(track) => {
                                                    if let Err(e) = state_clone
                                                        .playback_engine
                                                        .load_track(track, session.position_ms)
                                                        .await
                                                    {
                                                        warn!("handoff: failed to load track {tid} on takeover: {e}");
                                                        takeover_ok = false;
                                                    } else {
                                                        info!(
                                                            "handoff: takeover track={} at position={}",
                                                            tid, session.position_ms
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!("handoff: track {tid} not resolved locally for takeover: {e}");
                                                    takeover_ok = false;
                                                }
                                            }
                                        }
                                        if takeover_ok {
                                            if session.playing {
                                                if let Err(e) =
                                                    state_clone.playback_engine.resume().await
                                                {
                                                    warn!(
                                                        "handoff: failed to resume playback on takeover: {e}"
                                                    );
                                                }
                                            }
                                            let vol_u8 = ((session.volume * 100.0)
                                                .round()
                                                .clamp(0.0, 100.0))
                                                as u8;
                                            let _ = state_clone
                                                .playback_engine
                                                .set_volume(vol_u8)
                                                .await;

                                            let accept =
                                                michi_sync::SyncMessage::handoff_accept(session);
                                            let _ = state_clone.sync_tx.send(accept);
                                        } else {
                                            warn!(
                                                "handoff: takeover failed to apply state locally; refusing handoff_accept for {} -> {}",
                                                from_device, to_device
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            "handoff: initiate_handoff failed between {} and {}: {}",
                                            from_device, to_device, e
                                        );
                                    }
                                }
                            }
                            michi_sync::SyncMessage::HandoffAccept { session_data } => {
                                info!(
                                    "sync: handoff accepted at position {}",
                                    session_data.position_ms
                                );
                            }
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
