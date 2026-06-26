use std::sync::Arc;
use std::time::Duration;

use michi_config::Config;
use michi_sync::PlaybackState;
use rumqttc::{AsyncClient, MqttOptions, Packet, QoS};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const DISCOVERY_PREFIX: &str = "homeassistant";
const STATE_INTERVAL_SECS: u64 = 5;

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
            domain: "button",
            object_id: "play_pause",
            config: build_button_config("play_pause", "Michi Play/Pause", "mdi:play-pause"),
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
    ]
}

async fn publish_discovery(client: &AsyncClient) {
    for entity in entities() {
        let topic = format!(
            "{}/{}/michi_{}/config",
            DISCOVERY_PREFIX, entity.domain, entity.object_id
        );
        let payload = serde_json::to_string(&entity.config).unwrap();
        match client
            .publish(&topic, QoS::AtLeastOnce, true, payload)
            .await
        {
            Ok(_) => info!("published discovery for {}", entity.object_id),
            Err(e) => warn!(
                "failed to publish discovery for {}: {}",
                entity.object_id, e
            ),
        }
    }
}

async fn publish_states(
    client: &AsyncClient,
    playback_state: &Arc<RwLock<PlaybackState>>,
    db: &SqlitePool,
) {
    let state = playback_state.read().await;

    let (title, artist, album) = if let Some(track_id) = &state.track_id {
        match michi_db::get_track(db, track_id).await {
            Ok(Some(track)) => (
                track.title.unwrap_or_default(),
                track.artist.unwrap_or_default(),
                track.album.unwrap_or_default(),
            ),
            _ => (String::new(), String::new(), String::new()),
        }
    } else {
        (String::new(), String::new(), String::new())
    };

    let status = if state.playing { "playing" } else { "paused" };

    let states = [
        ("track_title", title),
        ("artist", artist),
        ("album", album),
        ("playback_status", status.to_string()),
    ];

    for (object_id, value) in &states {
        let topic = format!("michi/{}/state", object_id);
        if let Err(e) = client
            .publish(&topic, QoS::AtLeastOnce, true, value.clone())
            .await
        {
            warn!("failed to publish state for {}: {}", object_id, e);
        }
    }
}

async fn handle_command(topic: &str, config: &Config, playback_state: &Arc<RwLock<PlaybackState>>) {
    let cmd = topic.trim_start_matches("michi/").trim_end_matches("/cmd");

    info!("received command: {}", cmd);

    match cmd {
        "play_pause" => {
            let current = playback_state.read().await;
            let new_playing = !current.playing;
            drop(current);

            let url = format!("http://localhost:{}/api/playback/state", config.port);
            let body = json!({
                "playing": new_playing,
                "position_ms": 0,
                "track_id": null,
            });

            let client = reqwest::Client::new();
            match client.post(&url).json(&body).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!("play_pause toggled to {}", new_playing);
                    } else {
                        warn!(
                            "play_pause HTTP {} {}",
                            resp.status(),
                            resp.text().await.unwrap_or_default()
                        );
                    }
                }
                Err(e) => {
                    error!("play_pause HTTP request failed: {}", e);
                }
            }
        }
        "next_track" | "previous_track" => {
            let url = format!("http://localhost:{}/api/playback/state", config.port);
            let body = json!({
                "playing": false,
                "position_ms": 0,
                "track_id": null,
            });

            let client = reqwest::Client::new();
            match client.post(&url).json(&body).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!("{} executed", cmd);
                    } else {
                        warn!(
                            "{} HTTP {} {}",
                            cmd,
                            resp.status(),
                            resp.text().await.unwrap_or_default()
                        );
                    }
                }
                Err(e) => {
                    error!("{} HTTP request failed: {}", cmd, e);
                }
            }
        }
        _ => {
            warn!("unknown command: {}", cmd);
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

pub async fn run(config: Config, playback_state: Arc<RwLock<PlaybackState>>, db: SqlitePool) {
    let host = match std::env::var("MICHI_MQTT_HOST") {
        Ok(h) => h,
        Err(_) => {
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
    let client_id = format!("michi-{}", config.sync_name);

    loop {
        info!("connecting to MQTT broker at {}:{}", host, port);

        let (client, mut eventloop) =
            match mqtt_connect(&host, port, &user, &pass, &client_id).await {
                Ok(c) => c,
                Err(e) => {
                    error!("failed to create MQTT client: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

        publish_discovery(&client).await;

        for cmd in &["play_pause", "next_track", "previous_track"] {
            let topic = format!("michi/{}/cmd", cmd);
            if let Err(e) = client.subscribe(&topic, QoS::AtLeastOnce).await {
                warn!("failed to subscribe to {}: {}", topic, e);
            }
        }

        publish_states(&client, &playback_state, &db).await;
        info!("HA integration running");

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
                            handle_command(&topic, &config, &playback_state).await;
                        }
                    }
                    rumqttc::Event::Incoming(Packet::ConnAck(_)) => {
                        info!("MQTT connected/ reconnected");
                    }
                    _ => {}
                },
                Ok(Err(e)) => {
                    error!("MQTT error: {:?}", e);
                    break;
                }
                Err(_) => {
                    publish_states(&client, &playback_state, &db).await;
                    last_state_publish = tokio::time::Instant::now();
                }
            }
        }

        warn!("MQTT connection lost, reconnecting in 5 seconds...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
