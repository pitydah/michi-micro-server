//! Single source of truth for server capabilities (M1: ServerCapabilities consolidation).
//!
//! Previously `/api/v1/capabilities` and `/api/v1/server/info` computed their
//! feature sets independently, causing drift (e.g. `receivers` was hardcoded
//! `false` in one place while the other emitted a different feature list).
//! Both endpoints now derive from [`ServerCapabilities::from_state`].

use serde::Serialize;

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ServerFeature {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerProtocol {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerRuntime {
    pub receivers_connected: usize,
    pub ffmpeg_available: bool,
}

/// Canonical server capabilities manifest.
#[derive(Debug, Clone, Serialize)]
pub struct ServerCapabilities {
    pub version: String,
    pub features: Vec<ServerFeature>,
    pub protocols: Vec<ServerProtocol>,
    pub runtime: ServerRuntime,
}

const MODULE_FEATURES: &[(&str, &str, &str)] = &[
    ("scan", "1.0", "Library scanning with watcher"),
    ("sync", "1.0", "Peer-to-peer library sync"),
    ("stream", "1.0", "Direct & proxied audio streaming"),
    ("playback", "1.0", "Playback tracking & history"),
    ("backup", "1.0", "JSON backup & tar.gz bundle"),
    ("webhook", "1.0", "Sync completion webhooks"),
];

const ALWAYS_ON_FEATURES: &[(&str, &str, &str)] = &[
    ("etag", "1.0", "ETag-based conditional requests"),
    ("handoff", "1.0", "Direct stream handoff between peers"),
    ("mounts", "1.0", "Mount health monitoring"),
    ("audit", "1.0", "Audit log for admin actions"),
    ("jobs", "1.0", "Persistent job queue with workers"),
    ("modules", "1.0", "Runtime module enable/disable"),
    ("library", "1.0", "Library browsing"),
    ("search", "1.0", "Library search"),
    ("download", "1.0", "Track download"),
    ("artwork", "1.0", "Artwork serving"),
    ("playlists", "1.0", "Playlist management"),
    ("import", "1.0", "Library import"),
    ("queue", "1.0", "Playback queue"),
    ("events", "1.0", "Server-sent events"),
    ("token_refresh", "1.0", "Device token refresh"),
];

/// Features that are always disabled in the micro profile (kept for shape parity).
const DISABLED_FEATURES: &[(&str, &str, &str)] = &[
    ("receivers", "1.0", "Receiver playback groups"),
    ("rooms", "1.0", "Multi-room playback"),
];

impl ServerCapabilities {
    /// Build the canonical capabilities manifest from the live server state.
    ///
    /// This is the single source of truth: every endpoint that advertises
    /// server capabilities MUST derive its payload from this value.
    pub async fn from_state(state: &AppState) -> Self {
        let disabled = state.disabled_modules.read().await;
        let receiver_count = state
            .receiver_manager
            .registry()
            .await
            .read()
            .await
            .list()
            .len();
        let ffmpeg = michi_streaming::check_ffmpeg();

        let mut features: Vec<ServerFeature> = MODULE_FEATURES
            .iter()
            .map(|(name, version, description)| ServerFeature {
                name,
                version,
                description,
                enabled: !disabled.contains(*name),
            })
            .collect();
        features.extend(
            ALWAYS_ON_FEATURES
                .iter()
                .map(|(name, version, description)| ServerFeature {
                    name,
                    version,
                    description,
                    enabled: true,
                }),
        );
        features.extend(
            DISABLED_FEATURES
                .iter()
                .map(|(name, version, description)| ServerFeature {
                    name,
                    version,
                    description,
                    enabled: false,
                }),
        );
        features.push(ServerFeature {
            name: "transcoding",
            version: "1.0",
            description: "FFmpeg transcoding support",
            enabled: ffmpeg,
        });

        Self {
            version: state.config.version().to_string(),
            features,
            protocols: vec![
                ServerProtocol {
                    name: "michi-link",
                    version: "0.2",
                },
                ServerProtocol {
                    name: "websocket",
                    version: "1.0",
                },
            ],
            runtime: ServerRuntime {
                receivers_connected: receiver_count,
                ffmpeg_available: ffmpeg,
            },
        }
    }

    /// Static feature definitions (without runtime state), used for validation.
    #[cfg(test)]
    pub fn features_static() -> Vec<ServerFeature> {
        let mut features: Vec<ServerFeature> = MODULE_FEATURES
            .iter()
            .chain(ALWAYS_ON_FEATURES)
            .chain(DISABLED_FEATURES)
            .map(|(name, version, description)| ServerFeature {
                name,
                version,
                description,
                enabled: !DISABLED_FEATURES.iter().any(|(n, _, _)| n == name),
            })
            .collect();
        features.push(ServerFeature {
            name: "transcoding",
            version: "1.0",
            description: "FFmpeg transcoding support",
            enabled: false,
        });
        features
    }

    /// Look up whether a named feature is enabled.
    pub fn feature_enabled(&self, name: &str) -> bool {
        self.features
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.enabled)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_manifest_is_complete_and_non_duplicated() {
        let names: Vec<&str> = ServerCapabilities::features_static()
            .iter()
            .map(|f| f.name)
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "feature names must not be duplicated"
        );
        assert!(sorted.contains(&"stream"));
        assert!(sorted.contains(&"transcoding"));
        assert!(sorted.contains(&"token_refresh"));
    }

    #[test]
    fn feature_enabled_lookup() {
        let caps = ServerCapabilities {
            version: "0.0.0".into(),
            features: vec![
                ServerFeature {
                    name: "stream",
                    version: "1.0",
                    description: "d",
                    enabled: true,
                },
                ServerFeature {
                    name: "receivers",
                    version: "1.0",
                    description: "d",
                    enabled: false,
                },
            ],
            protocols: vec![],
            runtime: ServerRuntime {
                receivers_connected: 0,
                ffmpeg_available: false,
            },
        };
        assert!(caps.feature_enabled("stream"));
        assert!(!caps.feature_enabled("receivers"));
        assert!(!caps.feature_enabled("unknown"));
    }
}
