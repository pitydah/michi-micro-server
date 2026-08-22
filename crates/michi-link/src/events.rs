//! Canonical WebSocket event types and payloads for Michi Link v1 (§18).
//!
//! All real-time server events broadcasted via `/api/v1/events` (and `/api/ws`)
//! MUST serialize into this canonical wire structure:
//!
//! ```json
//! {
//!   "type": "playback.state_changed",
//!   "data": { ... }
//! }
//! ```

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Canonical event types emitted by Michi Server over WebSocket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum LinkEvent {
    /// Initial server state handshake upon connection / authentication.
    #[serde(rename = "server.status")]
    ServerStatus {
        service: String,
        version: String,
        server_id: Uuid,
    },
    /// Playback state changed (playing, paused, stopped, volume, position).
    #[serde(rename = "playback.state_changed")]
    PlaybackStateChanged {
        playing: bool,
        current_track_id: Option<Uuid>,
        position_ms: u64,
        volume: f64,
        repeat_mode: String,
        shuffle: bool,
    },
    /// Track currently playing changed.
    #[serde(rename = "playback.track_changed")]
    TrackChanged {
        track_id: Option<Uuid>,
        position_ms: u64,
    },
    /// Playback queue updated (items added, removed, reordered).
    #[serde(rename = "queue.changed")]
    QueueChanged {
        queue_id: Option<Uuid>,
        item_count: usize,
        current_index: i32,
    },
    /// Receiver online status or discovery state changed.
    #[serde(rename = "receiver.state_changed")]
    ReceiverStateChanged {
        receiver_id: Uuid,
        name: String,
        online: bool,
        has_active_session: bool,
    },
    /// Multi-room or Zone configuration changed.
    #[serde(rename = "zone.state_changed")]
    ZoneStateChanged {
        zone_id: String,
        name: String,
        volume: u8,
        muted: bool,
    },
    /// Library scan progress or completion.
    #[serde(rename = "library.scan_status")]
    LibraryScanStatus {
        scanning: bool,
        tracks_indexed: usize,
        message: Option<String>,
    },
}

impl LinkEvent {
    /// Serialize the event into the canonical JSON string for WebSocket transport.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_status_event_wire_format() {
        let sid = Uuid::nil();
        let ev = LinkEvent::ServerStatus {
            service: "michi-micro-server".into(),
            version: "0.2.0".into(),
            server_id: sid,
        };
        let json = ev.to_json_string().unwrap();
        assert!(json.contains("\"type\":\"server.status\""));
        assert!(json.contains("\"service\":\"michi-micro-server\""));
    }

    #[test]
    fn playback_state_changed_event_wire_format() {
        let tid = Uuid::new_v4();
        let ev = LinkEvent::PlaybackStateChanged {
            playing: true,
            current_track_id: Some(tid),
            position_ms: 45000,
            volume: 0.85,
            repeat_mode: "all".into(),
            shuffle: true,
        };
        let json = ev.to_json_string().unwrap();
        assert!(json.contains("\"type\":\"playback.state_changed\""));
        assert!(json.contains("\"playing\":true"));
        assert!(json.contains("\"position_ms\":45000"));
    }

    #[test]
    fn roundtrip_deserialization() {
        let ev = LinkEvent::QueueChanged {
            queue_id: Some(Uuid::nil()),
            item_count: 5,
            current_index: 2,
        };
        let json = ev.to_json_string().unwrap();
        let parsed: LinkEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ev);
    }
}
