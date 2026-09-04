use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use michi_playback::TrackResolver;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

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
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    State(state): State<AppState>,
    ws_result: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    let sync_lock = state.get_module_transition_lock("sync").await;
    let _sync_guard = sync_lock.lock().await;

    if state.disabled_modules.read().await.contains("sync") {
        warn!("sync_ws: rejected sync connection because sync module is disabled");
        return (StatusCode::SERVICE_UNAVAILABLE, "sync module is disabled").into_response();
    }

    let Some(sync_cancel) = state.module_tokens.read().await.get("sync").cloned() else {
        warn!("sync_ws: rejected sync connection because canonical sync token is missing");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "sync module runtime unavailable",
        )
            .into_response();
    };

    drop(_sync_guard);

    let client_ip = crate::resolve_client_ip(connect_info, &headers, &state.config);

    if !state.config.remote_sync {
        match client_ip {
            Some(ip) => {
                if !is_local_or_private_ip(ip) {
                    warn!(
                        "sync_ws: rejected non-local sync connection from {} (remote_sync=false)",
                        ip
                    );
                    return (
                        StatusCode::FORBIDDEN,
                        "Remote sync is disabled on this server",
                    )
                        .into_response();
                }
            }
            None => {
                warn!(
                    "sync_ws: rejected sync connection due to missing/unverifiable client connection info (remote_sync=false fail-closed)"
                );
                return (
                    StatusCode::FORBIDDEN,
                    "Remote sync is disabled and client IP cannot be verified",
                )
                    .into_response();
            }
        }
    }

    match ws_result {
        Ok(ws) => ws
            .on_upgrade(move |socket| handle_sync(socket, state, sync_cancel))
            .into_response(),
        Err(rejection) => rejection.into_response(),
    }
}

pub async fn apply_remote_playback_state(
    state: &AppState,
    track_id: Option<Uuid>,
    position_ms: u64,
    playing: bool,
    volume: f64,
) -> bool {
    let playback_lock = state.get_module_transition_lock("playback").await;
    let _playback_guard = playback_lock.lock().await;

    if state.disabled_modules.read().await.contains("playback") {
        debug!("sync: playback module disabled locally, skipping playback state application");
        return false;
    }

    let mut all_applied = true;
    if let Some(tid) = track_id {
        let resolver = michi_playback::SqliteTrackResolver::new(
            state.db.clone(),
            state.config.music_paths.clone(),
        );
        match resolver.get_track(tid).await {
            Ok(track) => {
                if let Err(e) = state.playback_engine.load_track(track, position_ms).await {
                    warn!("sync: failed to load track {tid}: {e}");
                    all_applied = false;
                }
            }
            Err(e) => {
                warn!("sync: track {tid} not found locally: {e}");
                all_applied = false;
            }
        }
    } else if let Err(e) = state.playback_engine.seek(position_ms).await {
        warn!("sync: failed to seek to {position_ms}ms: {e}");
        all_applied = false;
    }

    if all_applied {
        let play_res = if playing {
            state.playback_engine.resume().await
        } else {
            state.playback_engine.pause().await
        };
        if let Err(e) = play_res {
            warn!("sync: failed to transition playback state: {e}");
            all_applied = false;
        }
    }

    if all_applied {
        let vol_u8 = ((volume * 100.0).round().clamp(0.0, 100.0)) as u8;
        if let Err(e) = state.playback_engine.set_volume(vol_u8).await {
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
        let _ = state.tx.send(msg);
    }

    all_applied
}

async fn handle_sync(socket: WebSocket, state: AppState, sync_cancel: CancellationToken) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // 1. Subscribe to broadcast channel first so we never miss events emitted concurrently with snapshot
    let mut rx = state.sync_tx.subscribe();

    // 2. Send identify message
    let identify = michi_sync::SyncMessage::Identify {
        name: state.config.sync_name.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        device_type: michi_sync::DeviceType::Server,
    };
    if let Ok(json) = serde_json::to_string(&identify) {
        let _ = ws_sender.send(Message::Text(json)).await;
    }

    // 3. Send current playback snapshot immediately upon connection and initialize monotonic tracker
    let initial_playback_state = state.playback_state.read().await.clone();
    let mut last_sent_updated_at = initial_playback_state.updated_at;
    let initial_msg: michi_sync::SyncMessage = initial_playback_state.into();
    if let Ok(json) = serde_json::to_string(&initial_msg) {
        let _ = ws_sender.send(Message::Text(json)).await;
    }

    let sync_cancel_send = sync_cancel.clone();
    let shutdown_send = state.shutdown_token.clone();
    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = sync_cancel_send.cancelled() => break,
                _ = shutdown_send.cancelled() => break,
                msg = rx.recv() => {
                    match msg {
                        Ok(sync_msg) => {
                            if let michi_sync::SyncMessage::State { updated_at, .. } = &sync_msg {
                                if *updated_at <= last_sent_updated_at {
                                    // Skip stale or duplicate state captured by snapshot
                                    continue;
                                }
                                last_sent_updated_at = *updated_at;
                            }
                            if let Ok(json) = serde_json::to_string(&sync_msg) {
                                if ws_sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!("sync: ws sender lagged by {} messages", n);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    let state_clone = state.clone();
    let sync_cancel_recv = sync_cancel.clone();
    let shutdown_recv = state.shutdown_token.clone();
    let mut recv_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = sync_cancel_recv.cancelled() => break,
                _ = shutdown_recv.cancelled() => break,
                msg_opt = ws_receiver.next() => {
                    let Some(msg_res) = msg_opt else { break; };
                    let Ok(msg) = msg_res else { break; };
                    match msg {
                        Message::Text(text) => {
                            if let Ok(sync_msg) = serde_json::from_str::<michi_sync::SyncMessage>(&text) {
                                match &sync_msg {
                                    michi_sync::SyncMessage::State {
                                        track_id,
                                        position_ms,
                                        playing,
                                        volume,
                                        ..
                                    } => {
                                        apply_remote_playback_state(
                                            &state_clone,
                                            *track_id,
                                            *position_ms,
                                            *playing,
                                            *volume,
                                        )
                                        .await;
                                    }
                                    michi_sync::SyncMessage::Identify { name, .. } => {
                                        info!("sync: connected peer identified as '{}'", name);
                                    }
                                    michi_sync::SyncMessage::Ping => {}
                                    michi_sync::SyncMessage::Pong => {}
                                    michi_sync::SyncMessage::HandoffRequest {
                                        from_device,
                                        to_device,
                                    } => {
                                        info!(
                                            "sync: handoff request from {} to {}",
                                            from_device, to_device
                                        );

                                        let playback_lock = state_clone.get_module_transition_lock("playback").await;
                                        let _playback_guard = playback_lock.lock().await;

                                        if state_clone
                                            .disabled_modules
                                            .read()
                                            .await
                                            .contains("playback")
                                        {
                                            warn!("handoff: playback module disabled; refusing handoff offer for {} -> {}", from_device, to_device);
                                        } else {
                                            match state_clone
                                                .sync_manager
                                                .initiate_handoff(
                                                    from_device.clone(),
                                                    to_device.clone(),
                                                )
                                                .await
                                            {
                                                Ok(session) => {
                                                    let mut takeover_ok = true;
                                                    if let Some(ref tid) = session.track_id {
                                                        let resolver =
                                                            michi_playback::SqliteTrackResolver::new(
                                                                state_clone.db.clone(),
                                                                state_clone.config.music_paths.clone(),
                                                            );
                                                        match resolver.get_track(*tid).await {
                                                            Ok(track) => {
                                                                if let Err(e) = state_clone
                                                                    .playback_engine
                                                                    .load_track(
                                                                        track,
                                                                        session.position_ms,
                                                                    )
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
                                                        if let Err(e) = state_clone
                                                            .playback_engine
                                                            .set_volume(vol_u8)
                                                            .await
                                                        {
                                                            warn!("handoff: failed to set volume on takeover: {e}");
                                                            takeover_ok = false;
                                                        }
                                                    }

                                                    if takeover_ok {
                                                        // Validate snapshot convergence fail-closed
                                                        match state_clone.playback_engine.snapshot().await {
                                                            Ok(snap) => {
                                                                let track_matches =
                                                                    snap.track_id == session.track_id;
                                                                let play_matches =
                                                                    if session.track_id.is_none() {
                                                                        !snap.lifecycle.is_playing()
                                                                    } else {
                                                                        snap.lifecycle.is_playing()
                                                                            == session.playing
                                                                    };
                                                                let vol_matches = ((snap.volume as f64
                                                                    / 100.0)
                                                                    - session.volume)
                                                                    .abs()
                                                                    <= 0.05;
                                                                let pos_matches =
                                                                    if session.track_id.is_none() {
                                                                        true
                                                                    } else if session.playing {
                                                                        (snap.position_ms as i64
                                                                            - session.position_ms as i64)
                                                                            .abs()
                                                                            <= 3000
                                                                    } else {
                                                                        (snap.position_ms as i64
                                                                            - session.position_ms as i64)
                                                                            .abs()
                                                                            <= 1000
                                                                    };

                                                                if !track_matches
                                                                    || !play_matches
                                                                    || !vol_matches
                                                                    || !pos_matches
                                                                {
                                                                    warn!(
                                                                    "handoff: snapshot readback mismatch (expected track={:?}, playing={}, vol={:.2}, pos={}; got track={:?}, lifecycle={:?}, vol={}, pos={})",
                                                                    session.track_id, session.playing, session.volume, session.position_ms,
                                                                    snap.track_id, snap.lifecycle, snap.volume, snap.position_ms
                                                                );
                                                                    takeover_ok = false;
                                                                }
                                                            }
                                                            Err(e) => {
                                                                warn!(
                                                                "handoff: failed to obtain engine snapshot on takeover: {e}"
                                                            );
                                                                takeover_ok = false;
                                                            }
                                                        }
                                                    }

                                                    if takeover_ok {
                                                        let accept =
                                                            michi_sync::SyncMessage::handoff_accept(
                                                                session,
                                                            );
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
            }
        }
    });

    tokio::select! {
        _ = sync_cancel.cancelled() => {
            info!("sync_ws: sync module disabled, terminating active inbound connection");
        }
        _ = state.shutdown_token.cancelled() => {
            info!("sync_ws: server shutdown, terminating active inbound connection");
        }
        _ = (&mut send_task) => {},
        _ = (&mut recv_task) => {},
    }
    send_task.abort();
    recv_task.abort();
    let _ = send_task.await;
    let _ = recv_task.await;
}
