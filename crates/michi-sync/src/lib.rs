use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncMessage {
    #[serde(rename = "identify")]
    Identify { name: String, version: String },
    #[serde(rename = "state")]
    State {
        track_id: Option<Uuid>,
        position_ms: u64,
        playing: bool,
        volume: f64,
        updated_at: DateTime<Utc>,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: f64,
    pub updated_at: DateTime<Utc>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            track_id: None,
            position_ms: 0,
            playing: false,
            volume: 0.8,
            updated_at: Utc::now(),
        }
    }
}

impl From<PlaybackState> for SyncMessage {
    fn from(state: PlaybackState) -> Self {
        SyncMessage::State {
            track_id: state.track_id,
            position_ms: state.position_ms,
            playing: state.playing,
            volume: state.volume,
            updated_at: state.updated_at,
        }
    }
}

impl SyncMessage {
    pub fn serialize(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn deserialize(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_state() {
        let msg = SyncMessage::State {
            track_id: Some(Uuid::nil()),
            position_ms: 12345,
            playing: true,
            volume: 0.8,
            updated_at: Utc::now(),
        };
        let json = msg.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        match deserialized {
            SyncMessage::State {
                track_id,
                position_ms,
                playing,
                volume,
                ..
            } => {
                assert_eq!(track_id, Some(Uuid::nil()));
                assert_eq!(position_ms, 12345);
                assert!(playing);
                assert!((volume - 0.8).abs() < 0.001);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_identify() {
        let msg = SyncMessage::Identify {
            name: "Living Room".into(),
            version: "0.1.0".into(),
        };
        let json = msg.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        match deserialized {
            SyncMessage::Identify { name, version } => {
                assert_eq!(name, "Living Room");
                assert_eq!(version, "0.1.0");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_ping() {
        let msg = SyncMessage::Ping;
        let json = msg.serialize().unwrap();
        let deserialized = SyncMessage::deserialize(&json).unwrap();
        assert!(matches!(deserialized, SyncMessage::Ping));
    }

    #[test]
    fn test_playback_state_default() {
        let state = PlaybackState::default();
        assert!(state.track_id.is_none());
        assert!(!state.playing);
        assert_eq!(state.position_ms, 0);
    }

    #[test]
    fn test_playback_state_into_sync_message() {
        let state = PlaybackState::default();
        let msg: SyncMessage = state.into();
        assert!(matches!(msg, SyncMessage::State { .. }));
    }
}
