use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use michi_playback::TrackResolver;
use tracing::{info, warn};

use crate::AppState;

fn is_local_or_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.octets()[0] == 10
                || (v4.octets()[0] == 172 && (16..=31).contains(&v4.octets()[1]))
                || (v4.octets()[0] == 192 && v4.octets()[1] == 168)
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unicast_link_local() || v6.is_unique_local(),
    }
}

pub async fn sync_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    State(state): State<AppState>,
) -> Response {
    if !state.config.remote_sync {
        let client_ip = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
            .or_else(|| connect_info.map(|ci| ci.0.ip()));

        if let Some(ip) = client_ip {
            if !is_local_or_private_ip(ip) {
                warn!("sync_ws: rejected remote sync connection from {ip} because remote_sync is disabled");
                return (
                    StatusCode::FORBIDDEN,
                    "Remote sync is disabled on this server",
                )
                    .into_response();
            }
        }
    }
    ws.on_upgrade(move |socket| handle_sync(socket, state))
        .into_response()
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
                                        } else if let Err(e) =
                                            state_clone.playback_engine.stop().await
                                        {
                                            warn!("handoff: failed to stop playback on empty track takeover: {e}");
                                            takeover_ok = false;
                                        }

                                        if takeover_ok && session.track_id.is_some() {
                                            let play_res = if session.playing {
                                                state_clone.playback_engine.resume().await
                                            } else {
                                                state_clone.playback_engine.pause().await
                                            };
                                            if let Err(e) = play_res {
                                                warn!("handoff: failed to transition playback state on takeover: {e}");
                                                takeover_ok = false;
                                            }
                                        }

                                        if takeover_ok {
                                            let vol_u8 = ((session.volume * 100.0)
                                                .round()
                                                .clamp(0.0, 100.0))
                                                as u8;
                                            if let Err(e) =
                                                state_clone.playback_engine.set_volume(vol_u8).await
                                            {
                                                warn!("handoff: failed to set volume on takeover: {e}");
                                                takeover_ok = false;
                                            }
                                        }

                                        if takeover_ok {
                                            // Validate snapshot convergence
                                            if let Ok(snap) =
                                                state_clone.playback_engine.snapshot().await
                                            {
                                                let track_matches =
                                                    snap.track_id == session.track_id;
                                                let play_matches = if session.track_id.is_none() {
                                                    !snap.lifecycle.is_playing()
                                                } else {
                                                    snap.lifecycle.is_playing() == session.playing
                                                };
                                                if !track_matches || !play_matches {
                                                    warn!(
                                                        "handoff: snapshot readback mismatch (expected track={:?}, playing={}; got track={:?}, lifecycle={:?})",
                                                        session.track_id, session.playing, snap.track_id, snap.lifecycle
                                                    );
                                                    takeover_ok = false;
                                                }
                                            }
                                        }

                                        if takeover_ok {
                                            let accept =
                                                michi_sync::SyncMessage::handoff_accept(session);
                                            let _ = state_clone.sync_tx.send(accept);
                                        } else {
                                            warn!(
                                                "handoff: takeover failed to apply or converge state locally; refusing handoff_accept for {} -> {}",
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
