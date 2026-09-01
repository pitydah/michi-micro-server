use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use michi_config::Config;
use michi_playback::{PlaybackEngineHandle, PlaybackLifecycle};
use rumqttc::{AsyncClient, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaRuntimeStatus {
    pub enabled: bool,
    pub configured: bool,
    pub connected: bool,
    pub broker: Option<String>,
    pub discovery_published: bool,
    pub last_published_at: Option<String>,
    pub last_error: Option<String>,
}

static HA_STATUS: OnceLock<RwLock<HaRuntimeStatus>> = OnceLock::new();

fn ha_status_store() -> &'static RwLock<HaRuntimeStatus> {
    HA_STATUS.get_or_init(|| {
        RwLock::new(HaRuntimeStatus {
            enabled: true,
            configured: false,
            connected: false,
            broker: None,
            discovery_published: false,
            last_published_at: None,
            last_error: None,
        })
    })
}

pub fn get_runtime_status() -> HaRuntimeStatus {
    ha_status_store().read().unwrap().clone()
}

pub fn update_runtime_status<F>(f: F)
where
    F: FnOnce(&mut HaRuntimeStatus),
{
    if let Ok(mut status) = ha_status_store().write() {
        f(&mut status);
    }
}

const DISCOVERY_PREFIX: &str = "homeassistant";
const STATE_INTERVAL_SECS: u64 = 2;

fn build_sensor_config(object_id: &str, name: &str, icon: &str) -> Value {
    json!({
        "name": name,
        "unique_id": format!("michi_{}", object_id),
        "state_topic": format!("michi/{}/state", object_id),
        "icon": icon,
    })
}

fn build_button_config(object_id: &str, name: &str, icon: &str) -> Value {
    json!({
        "name": name,
        "unique_id": format!("michi_{}", object_id),
        "command_topic": format!("michi/{}/cmd", object_id),
        "icon": icon,
        "payload_press": "",
    })
}

struct HaEntity {
    domain: &'static str,
    object_id: &'static str,
    config: Value,
}

fn entities() -> Vec<HaEntity> {
    vec![
        HaEntity {
            domain: "sensor",
            object_id: "track_title",
            config: build_sensor_config("track_title", "Michi Track Title", "mdi:music"),
        },
        HaEntity {
            domain: "sensor",
            object_id: "artist",
            config: build_sensor_config("artist", "Michi Artist", "mdi:account-music"),
        },
        HaEntity {
            domain: "sensor",
            object_id: "album",
            config: build_sensor_config("album", "Michi Album", "mdi:album"),
        },
        HaEntity {
            domain: "sensor",
            object_id: "playback_status",
            config: build_sensor_config(
                "playback_status",
                "Michi Playback Status",
                "mdi:play-pause",
            ),
        },
        HaEntity {
            domain: "sensor",
            object_id: "volume",
            config: build_sensor_config("volume", "Michi Volume", "mdi:volume-high"),
        },
        HaEntity {
            domain: "sensor",
            object_id: "track_duration",
            config: build_sensor_config(
                "track_duration",
                "Michi Track Duration",
                "mdi:timer-outline",
            ),
        },
        HaEntity {
            domain: "sensor",
            object_id: "playback_position",
            config: build_sensor_config(
                "playback_position",
                "Michi Playback Position",
                "mdi:timer-play-outline",
            ),
        },
        HaEntity {
            domain: "sensor",
            object_id: "server_status",
            config: build_sensor_config("server_status", "Michi Server Status", "mdi:server"),
        },
        HaEntity {
            domain: "button",
            object_id: "play_pause",
            config: build_button_config("play_pause", "Michi Play/Pause", "mdi:play-pause"),
        },
        HaEntity {
            domain: "button",
            object_id: "play",
            config: build_button_config("play", "Michi Play", "mdi:play"),
        },
        HaEntity {
            domain: "button",
            object_id: "pause",
            config: build_button_config("pause", "Michi Pause", "mdi:pause"),
        },
        HaEntity {
            domain: "button",
            object_id: "stop",
            config: build_button_config("stop", "Michi Stop", "mdi:stop"),
        },
        HaEntity {
            domain: "button",
            object_id: "next_track",
            config: build_button_config("next_track", "Michi Next Track", "mdi:skip-next"),
        },
        HaEntity {
            domain: "button",
            object_id: "previous_track",
            config: build_button_config(
                "previous_track",
                "Michi Previous Track",
                "mdi:skip-previous",
            ),
        },
        HaEntity {
            domain: "number",
            object_id: "volume_set",
            config: json!({
                "name": "Michi Volume Set",
                "unique_id": "michi_volume_set",
                "command_topic": "michi/volume_set/cmd",
                "state_topic": "michi/volume_set/state",
                "icon": "mdi:volume-high",
                "min": 0,
                "max": 100,
                "step": 5,
            }),
        },
    ]
}

async fn publish_discovery(client: &AsyncClient) -> Result<(), String> {
    let mut err_count = 0;
    let mut last_err = String::new();
    for entity in entities() {
        let topic = format!(
            "{}/{}/michi_{}/config",
            DISCOVERY_PREFIX, entity.domain, entity.object_id
        );
        let payload = match serde_json::to_string(&entity.config) {
            Ok(p) => p,
            Err(e) => {
                let msg = format!(
                    "failed to serialize entity config for {}: {e}",
                    entity.object_id
                );
                error!("{msg}");
                err_count += 1;
                last_err = msg;
                continue;
            }
        };
        match client
            .publish(&topic, QoS::AtLeastOnce, true, payload)
            .await
        {
            Ok(_) => info!("published discovery for {}", entity.object_id),
            Err(e) => {
                let msg = format!(
                    "failed to publish discovery for {}: {}",
                    entity.object_id, e
                );
                warn!("{msg}");
                err_count += 1;
                last_err = msg;
            }
        }
    }
    if err_count > 0 {
        Err(format!(
            "Discovery publication incomplete ({err_count} errors, last: {last_err})"
        ))
    } else {
        Ok(())
    }
}

async fn publish_states(
    client: &AsyncClient,
    engine: &PlaybackEngineHandle,
    db: &SqlitePool,
) -> Result<(), String> {
    let snap = match engine.snapshot().await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("failed to get engine snapshot for HA: {e}");
            warn!("{msg}");
            return Err(msg);
        }
    };

    let (title, artist, album, track_duration_ms) = if let Some(ref track_id) = snap.track_id {
        match michi_db::get_track(db, track_id).await {
            Ok(Some(track)) => (
                track.title.unwrap_or_default(),
                track.artist.unwrap_or_default(),
                track.album.unwrap_or_default(),
                track.duration_ms.unwrap_or(0),
            ),
            _ => (String::new(), String::new(), String::new(), 0),
        }
    } else {
        (String::new(), String::new(), String::new(), 0)
    };

    let status = snap.lifecycle.as_str();
    let volume_pct = snap.volume as u32;

    let states = [
        ("track_title", title),
        ("artist", artist),
        ("album", album),
        ("playback_status", status.to_string()),
        ("volume", volume_pct.to_string()),
        ("track_duration", track_duration_ms.to_string()),
        ("playback_position", snap.position_ms.to_string()),
        ("server_status", "online".to_string()),
        ("volume_set", volume_pct.to_string()),
    ];

    let mut err_count = 0;
    let mut last_err = String::new();
    for (object_id, value) in &states {
        let topic = format!("michi/{object_id}/state");
        if let Err(e) = client
            .publish(&topic, QoS::AtLeastOnce, true, value.clone())
            .await
        {
            let msg = format!("failed to publish state for {object_id}: {e}");
            warn!("{msg}");
            err_count += 1;
            last_err = msg;
        }
    }
    if err_count > 0 {
        Err(format!(
            "State publication incomplete ({err_count} errors, last: {last_err})"
        ))
    } else {
        Ok(())
    }
}

async fn handle_command(topic: &str, payload: &str, engine: &PlaybackEngineHandle) {
    let cmd = topic.trim_start_matches("michi/").trim_end_matches("/cmd");

    info!("received HA MQTT command: {} (payload: '{}')", cmd, payload);

    match cmd {
        "play_pause" => {
            if let Ok(snap) = engine.snapshot().await {
                let is_flowing = matches!(
                    snap.lifecycle,
                    PlaybackLifecycle::AudioFlowing | PlaybackLifecycle::Playing
                );
                if is_flowing {
                    if let Err(e) = engine.pause().await {
                        warn!("HA play_pause -> pause error: {e}");
                    }
                } else if let Err(e) = engine.resume().await {
                    warn!("HA play_pause -> resume error: {e}");
                }
            }
        }
        "play" => {
            if let Err(e) = engine.resume().await {
                warn!("HA play -> resume error: {e}");
            }
        }
        "pause" => {
            if let Err(e) = engine.pause().await {
                warn!("HA pause error: {e}");
            }
        }
        "stop" => {
            if let Err(e) = engine.stop().await {
                warn!("HA stop error: {e}");
            }
        }
        "next_track" | "next" => {
            if let Err(e) = engine.next().await {
                warn!("HA next error: {e}");
            }
        }
        "previous_track" | "previous" => {
            if let Err(e) = engine.previous().await {
                warn!("HA previous error: {e}");
            }
        }
        "volume_set" => {
            if let Ok(val) = payload.trim().parse::<f64>() {
                let vol_u8 = (val.round() as u8).min(100);
                if let Err(e) = engine.set_volume(vol_u8).await {
                    warn!("HA volume_set error: {e}");
                }
            } else if let Ok(val) = payload.trim().parse::<u8>() {
                if let Err(e) = engine.set_volume(val.min(100)).await {
                    warn!("HA volume_set error: {e}");
                }
            } else {
                warn!("HA volume_set invalid payload: {}", payload);
            }
        }
        _ => {
            warn!("unknown HA command: {}", cmd);
        }
    }
}

async fn mqtt_connect(
    host: &str,
    port: u16,
    user: &Option<String>,
    pass: &Option<String>,
    client_id: &str,
) -> Result<(AsyncClient, rumqttc::EventLoop), rumqttc::ClientError> {
    let mut mqtt_opts = MqttOptions::new(client_id, host, port);
    mqtt_opts.set_keep_alive(Duration::from_secs(30));
    mqtt_opts.set_clean_session(true);
    if let (Some(u), Some(p)) = (user, pass) {
        mqtt_opts.set_credentials(u, p);
    }

    let (client, eventloop) = AsyncClient::new(mqtt_opts, 100);
    Ok((client, eventloop))
}

pub async fn run(config: Config, engine: PlaybackEngineHandle, db: SqlitePool) {
    let host = match std::env::var("MICHI_MQTT_HOST") {
        Ok(h) => h,
        Err(_) => {
            update_runtime_status(|s| {
                s.configured = false;
                s.connected = false;
                s.broker = None;
            });
            error!("MICHI_MQTT_HOST not set, HA integration disabled");
            return;
        }
    };
    let port: u16 = std::env::var("MICHI_MQTT_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1883);
    let user = std::env::var("MICHI_MQTT_USER").ok();
    let pass = std::env::var("MICHI_MQTT_PASS").ok();
    let client_id = format!("michi_micro_{}", &config.server_id.to_string()[..8]);
    let broker_str = format!("{host}:{port}");

    update_runtime_status(|s| {
        s.configured = true;
        s.connected = false;
        s.discovery_published = false;
        s.broker = Some(broker_str.clone());
        s.last_error = None;
    });

    loop {
        info!("connecting to MQTT broker at {}:{}", host, port);

        let (client, mut eventloop) =
            match mqtt_connect(&host, port, &user, &pass, &client_id).await {
                Ok(c) => c,
                Err(e) => {
                    update_runtime_status(|s| {
                        s.connected = false;
                        s.discovery_published = false;
                        s.last_error = Some(e.to_string());
                    });
                    error!("failed to create MQTT client: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

        for cmd in &[
            "play_pause",
            "play",
            "pause",
            "stop",
            "next_track",
            "previous_track",
            "volume_set",
        ] {
            let topic = format!("michi/{cmd}/cmd");
            if let Err(e) = client.subscribe(&topic, QoS::AtLeastOnce).await {
                warn!("failed to subscribe to {}: {}", topic, e);
            }
        }

        info!("HA integration client initialized; waiting for broker ConnAck");
        let mut last_state_publish = tokio::time::Instant::now();

        loop {
            let timeout = Duration::from_secs(STATE_INTERVAL_SECS)
                .checked_sub(last_state_publish.elapsed())
                .unwrap_or(Duration::ZERO);

            match tokio::time::timeout(timeout, eventloop.poll()).await {
                Ok(Ok(notification)) => match notification {
                    rumqttc::Event::Incoming(Packet::Publish(publish)) => {
                        let topic = publish.topic;
                        let payload = String::from_utf8_lossy(&publish.payload).to_string();
                        info!(
                            "received MQTT message on {}: {}",
                            &topic,
                            payload.chars().take(100).collect::<String>()
                        );
                        if topic.starts_with("michi/") {
                            handle_command(&topic, &payload, &engine).await;
                            // Immediately publish updated states after command execution
                            if publish_states(&client, &engine, &db).await.is_ok() {
                                update_runtime_status(|s| {
                                    s.last_published_at = Some(chrono::Utc::now().to_rfc3339());
                                });
                            }
                            last_state_publish = tokio::time::Instant::now();
                        }
                    }
                    rumqttc::Event::Incoming(Packet::ConnAck(_)) => {
                        info!("MQTT connected/reconnected");
                        let disc_res = publish_discovery(&client).await;
                        let state_res = publish_states(&client, &engine, &db).await;
                        update_runtime_status(|s| {
                            s.connected = true;
                            match disc_res {
                                Ok(()) => {
                                    s.discovery_published = true;
                                    s.last_error = None;
                                }
                                Err(ref e) => {
                                    s.discovery_published = false;
                                    s.last_error = Some(e.clone());
                                }
                            }
                            if state_res.is_ok() {
                                s.last_published_at = Some(chrono::Utc::now().to_rfc3339());
                            }
                        });
                    }
                    rumqttc::Event::Incoming(Packet::PubAck(puback)) => {
                        debug!("MQTT broker acknowledged packet (pkid: {})", puback.pkid);
                        update_runtime_status(|s| {
                            if s.connected {
                                s.discovery_published = true;
                                s.last_published_at = Some(chrono::Utc::now().to_rfc3339());
                            }
                        });
                    }
                    rumqttc::Event::Incoming(Packet::Disconnect) => {
                        info!("MQTT broker disconnected");
                        update_runtime_status(|s| {
                            s.connected = false;
                            s.discovery_published = false;
                        });
                    }
                    _ => {}
                },
                Ok(Err(e)) => {
                    update_runtime_status(|s| {
                        s.connected = false;
                        s.discovery_published = false;
                        s.last_error = Some(format!("{e:?}"));
                    });
                    error!("MQTT error: {:?}", e);
                    break;
                }
                Err(_) => {
                    if get_runtime_status().connected
                        && publish_states(&client, &engine, &db).await.is_ok()
                    {
                        update_runtime_status(|s| {
                            s.last_published_at = Some(chrono::Utc::now().to_rfc3339());
                        });
                    }
                    last_state_publish = tokio::time::Instant::now();
                }
            }
        }

        update_runtime_status(|s| {
            s.connected = false;
            s.discovery_published = false;
        });
        warn!("MQTT connection lost, reconnecting in 5 seconds...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entities_count() {
        let ents = entities();
        assert!(ents.len() >= 12, "expected sensors + buttons + numbers");
    }

    #[test]
    fn test_discovery_prefix() {
        assert_eq!(DISCOVERY_PREFIX, "homeassistant");
    }

    #[test]
    fn test_entities_config_serializable() {
        for entity in entities() {
            let payload = serde_json::to_string(&entity.config);
            assert!(
                payload.is_ok(),
                "entity {} config should serialize",
                entity.object_id
            );
        }
    }
}
