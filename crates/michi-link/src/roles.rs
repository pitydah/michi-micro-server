//! Canonical server roles for Michi Link v1.
//!
//! These are the ONLY valid role strings that may appear in the `roles` field of
//! `GET /api/v1/server/info`. Implementors MUST use these constants — never
//! invent ad-hoc strings — to prevent silent contract drift between server
//! implementations and client parsers.
//!
//! # Contract authority
//! This module is the normative source for role names in the Michi ecosystem.
//! Both Micro Server and any future Big Server implementation MUST derive their
//! `roles` arrays from [`ServerRole::as_str`] / [`CANONICAL_MICRO_ROLES`].

use serde::{Deserialize, Serialize};

/// Canonical roles a Michi server may advertise via the Michi Link v1 contract.
///
/// Only these values are valid in `GET /api/v1/server/info → roles`.
/// The `#[serde(rename_all = "snake_case")]` ensures wire format stability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerRole {
    /// Server hosts and indexes a music library.
    MusicServer,
    /// Server is authoritative for playback sessions and queue management.
    PlaybackHost,
    /// Server acts as a receiver controller (manages audio endpoints).
    ReceiverController,
    /// Server supports multiroom / zone grouping.
    RoomController,
    /// Server acts as a sync peer for library manifest exchange.
    SyncPeer,
}

impl ServerRole {
    /// Returns the canonical wire-format string for this role.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MusicServer => "music_server",
            Self::PlaybackHost => "playback_host",
            Self::ReceiverController => "receiver_controller",
            Self::RoomController => "room_controller",
            Self::SyncPeer => "sync_peer",
        }
    }
}

impl std::fmt::Display for ServerRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical roles advertised by Michi **Micro** Server.
///
/// Use this constant as the authoritative role set when building `server/info`
/// responses. Do NOT hard-code string literals in route handlers.
pub const CANONICAL_MICRO_ROLES: &[ServerRole] = &[
    ServerRole::MusicServer,
    ServerRole::PlaybackHost,
    ServerRole::ReceiverController,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_serialize_to_canonical_strings() {
        assert_eq!(ServerRole::MusicServer.as_str(), "music_server");
        assert_eq!(ServerRole::PlaybackHost.as_str(), "playback_host");
        assert_eq!(
            ServerRole::ReceiverController.as_str(),
            "receiver_controller"
        );
        assert_eq!(ServerRole::RoomController.as_str(), "room_controller");
        assert_eq!(ServerRole::SyncPeer.as_str(), "sync_peer");
    }

    #[test]
    fn canonical_micro_roles_are_stable() {
        let strs: Vec<&str> = CANONICAL_MICRO_ROLES.iter().map(|r| r.as_str()).collect();
        assert!(strs.contains(&"music_server"));
        assert!(strs.contains(&"playback_host"));
        assert!(strs.contains(&"receiver_controller"));
        // Micro does NOT claim room_controller by default
        assert!(!strs.contains(&"room_controller"));
    }

    #[test]
    fn roles_round_trip_via_serde_json() {
        let role = ServerRole::MusicServer;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"music_server\"");
        let back: ServerRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }
}
