//! Michi Link integration adapter for server roles.
//!
//! # Architecture Authority
//! The normative specification and schema authority for Michi Link resides in
//! `pitydah/michi-link` (pinned in `vendor/michi-link`). This crate acts as an
//! internal adapter/runtime integration layer and derives its roles from
//! [`michi_identity::types::Role`].

pub use michi_identity::types::Role as ServerRole;

/// Canonical roles advertised by Michi **Micro** Server per `server-info.schema.json`.
///
/// Per the official Michi Link v1 contract for `michi-micro-server`:
/// `["music_server", "library_host", "playback_host"]`.
pub const CANONICAL_MICRO_ROLES: &[ServerRole] = &[
    ServerRole::MusicServer,
    ServerRole::LibraryHost,
    ServerRole::PlaybackHost,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_serialize_to_canonical_strings() {
        assert_eq!(ServerRole::MusicServer.as_str(), "music_server");
        assert_eq!(ServerRole::LibraryHost.as_str(), "library_host");
        assert_eq!(ServerRole::PlaybackHost.as_str(), "playback_host");
        assert_eq!(ServerRole::MobilePlayer.as_str(), "mobile_player");
        assert_eq!(ServerRole::RemoteController.as_str(), "remote_controller");
        assert_eq!(ServerRole::AudioReceiver.as_str(), "audio_receiver");
    }

    #[test]
    fn canonical_micro_roles_match_exact_contract() {
        let strs: Vec<&str> = CANONICAL_MICRO_ROLES.iter().map(|r| r.as_str()).collect();
        assert_eq!(
            strs,
            vec!["music_server", "library_host", "playback_host"],
            "Micro server roles must strictly match server-info.schema.json"
        );
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
