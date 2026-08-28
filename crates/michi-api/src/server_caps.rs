//! Single source of truth for server capabilities with truthful maturity and evidence levels.

use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureMaturity {
    Stable,
    Beta,
    Experimental,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    Declared,
    Wired,
    Implemented,
    EffectVerified,
    IntegrationCertified,
    HardwareCertified,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerFeature {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub enabled: bool,
    pub maturity: FeatureMaturity,
    pub evidence: EvidenceLevel,
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

const MODULE_FEATURES: &[(&str, &str, &str, FeatureMaturity, EvidenceLevel)] = &[
    (
        "scan",
        "1.0",
        "Library scanning with watcher",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "sync",
        "1.0",
        "Peer-to-peer library sync",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "stream",
        "1.0",
        "Direct & proxied audio streaming",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "playback",
        "1.0",
        "Playback tracking & history",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "backup",
        "1.0",
        "JSON backup & tar.gz bundle",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "webhook",
        "1.0",
        "Sync completion webhooks",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "receivers",
        "1.0",
        "Receiver playback data plane",
        FeatureMaturity::Beta,
        EvidenceLevel::IntegrationCertified,
    ),
    (
        "rooms",
        "1.0",
        "Multi-room playback routing",
        FeatureMaturity::Beta,
        EvidenceLevel::Implemented,
    ),
];

const ALWAYS_ON_FEATURES: &[(&str, &str, &str, FeatureMaturity, EvidenceLevel)] = &[
    (
        "etag",
        "1.0",
        "ETag-based conditional requests",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "handoff",
        "1.0",
        "Direct stream handoff between peers",
        FeatureMaturity::Beta,
        EvidenceLevel::EffectVerified,
    ),
    (
        "mounts",
        "1.0",
        "Mount health monitoring",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "audit",
        "1.0",
        "Audit log for admin actions",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "jobs",
        "1.0",
        "Persistent job queue with workers",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "modules",
        "1.0",
        "Runtime module enable/disable",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "library",
        "1.0",
        "Library browsing",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "search",
        "1.0",
        "Library search",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "download",
        "1.0",
        "Track download",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "artwork",
        "1.0",
        "Artwork serving",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "playlists",
        "1.0",
        "Playlist management",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "import",
        "1.0",
        "Library import",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "queue",
        "1.0",
        "Playback queue",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "events",
        "1.0",
        "Real-time WebSocket event dispatch",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "token_refresh",
        "1.0",
        "Device token refresh",
        FeatureMaturity::Stable,
        EvidenceLevel::EffectVerified,
    ),
    (
        "autonomous_playback",
        "1.0",
        "Autonomous decoding & engine playback",
        FeatureMaturity::Beta,
        EvidenceLevel::EffectVerified,
    ),
];

const DISABLED_FEATURES: &[(&str, &str, &str, FeatureMaturity, EvidenceLevel)] = &[];

impl ServerCapabilities {
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
            .map(
                |(name, version, description, maturity, evidence)| ServerFeature {
                    name,
                    version,
                    description,
                    enabled: !disabled.contains(*name),
                    maturity: *maturity,
                    evidence: *evidence,
                },
            )
            .collect();

        features.extend(ALWAYS_ON_FEATURES.iter().map(
            |(name, version, description, maturity, evidence)| {
                let enabled = if *name == "autonomous_playback" {
                    ffmpeg && !disabled.contains("playback")
                } else {
                    true
                };

                ServerFeature {
                    name,
                    version,
                    description,
                    enabled,
                    maturity: *maturity,
                    evidence: *evidence,
                }
            },
        ));

        features.extend(DISABLED_FEATURES.iter().map(
            |(name, version, description, maturity, evidence)| ServerFeature {
                name,
                version,
                description,
                enabled: false,
                maturity: *maturity,
                evidence: *evidence,
            },
        ));

        ServerCapabilities {
            version: env!("CARGO_PKG_VERSION").to_string(),
            features,
            protocols: vec![
                ServerProtocol {
                    name: "opensubsonic",
                    version: "1.16.1",
                },
                ServerProtocol {
                    name: "michi-link",
                    version: "1.0",
                },
                ServerProtocol {
                    name: "homeassistant",
                    version: "2024.1",
                },
            ],
            runtime: ServerRuntime {
                receivers_connected: receiver_count,
                ffmpeg_available: ffmpeg,
            },
        }
    }

    pub fn feature_enabled(&self, name: &str) -> bool {
        self.features
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.enabled)
            .unwrap_or(false)
    }

    pub fn to_features_map(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for feat in &self.features {
            map.insert(feat.name.to_string(), serde_json::Value::Bool(feat.enabled));
        }
        serde_json::Value::Object(map)
    }

    pub fn to_caps_map(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for feat in &self.features {
            map.insert(
                feat.name.to_string(),
                serde_json::json!({
                    "enabled": feat.enabled,
                    "version": feat.version,
                    "maturity": feat.maturity,
                    "evidence": feat.evidence,
                    "description": feat.description,
                }),
            );
        }
        serde_json::Value::Object(map)
    }

    pub fn discovery_features(&self) -> std::collections::BTreeMap<String, bool> {
        let mut map = std::collections::BTreeMap::new();
        map.insert("library".to_string(), self.feature_enabled("library"));
        map.insert("stream".to_string(), self.feature_enabled("stream"));
        map.insert("playback".to_string(), self.feature_enabled("playback"));
        map.insert("sync".to_string(), self.feature_enabled("sync"));
        map
    }
}
