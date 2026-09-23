//! Runtime projection of canonical product truth.
//! Product maturity is governed by spec/v1/product-truth.json.

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

// BEGIN GENERATED PRODUCT MATURITY
const CANONICAL_MATURITY: &[(&str, FeatureMaturity)] = &[
    ("adaptive_hls", FeatureMaturity::Unavailable),
    ("autonomous_playback", FeatureMaturity::Beta),
    ("backup", FeatureMaturity::Stable),
    ("gapless", FeatureMaturity::Unavailable),
    ("handoff", FeatureMaturity::Beta),
    ("hls_vod", FeatureMaturity::Stable),
    ("library", FeatureMaturity::Stable),
    ("opensubsonic", FeatureMaturity::Beta),
    ("playback_history", FeatureMaturity::Stable),
    ("playlists", FeatureMaturity::Stable),
    ("receivers", FeatureMaturity::Beta),
    ("rooms", FeatureMaturity::Beta),
    ("search", FeatureMaturity::Stable),
    ("security", FeatureMaturity::Stable),
    ("stream", FeatureMaturity::Stable),
    ("sync", FeatureMaturity::Stable),
    ("transcode", FeatureMaturity::Stable),
];
// END GENERATED PRODUCT MATURITY

fn canonical_maturity(name: &str, fallback: FeatureMaturity) -> FeatureMaturity {
    CANONICAL_MATURITY
        .iter()
        .find_map(|(feature, maturity)| (*feature == name).then_some(*maturity))
        .unwrap_or(fallback)
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
    (
        "transcode",
        "1.0",
        "On-demand FFmpeg audio transcoding",
        FeatureMaturity::Stable,
        EvidenceLevel::IntegrationCertified,
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
        Self::from_parts(&disabled, receiver_count, ffmpeg)
    }

    pub fn from_parts(
        disabled: &std::collections::HashSet<String>,
        receiver_count: usize,
        ffmpeg: bool,
    ) -> Self {
        let mut features: Vec<ServerFeature> = MODULE_FEATURES
            .iter()
            .map(
                |(name, version, description, maturity, evidence)| ServerFeature {
                    name,
                    version,
                    description,
                    enabled: !disabled.contains(*name),
                    maturity: canonical_maturity(name, *maturity),
                    evidence: *evidence,
                },
            )
            .collect();

        features.extend(ALWAYS_ON_FEATURES.iter().map(
            |(name, version, description, maturity, evidence)| {
                let enabled = if *name == "autonomous_playback" {
                    ffmpeg && !disabled.contains("playback")
                } else if *name == "transcode" {
                    ffmpeg && !disabled.contains("stream")
                } else {
                    true
                };

                ServerFeature {
                    name,
                    version,
                    description,
                    enabled,
                    maturity: canonical_maturity(name, *maturity),
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
                maturity: canonical_maturity(name, *maturity),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_transcode_canonical_feature_materialization() {
        let disabled = HashSet::new();
        let caps = ServerCapabilities::from_parts(&disabled, 0, true);

        let transcode_feats: Vec<_> = caps
            .features
            .iter()
            .filter(|f| f.name == "transcode")
            .collect();
        assert_eq!(
            transcode_feats.len(),
            1,
            "features must contain exactly one 'transcode' feature"
        );

        let feat = transcode_feats[0];
        assert_eq!(feat.name, "transcode");
        assert_eq!(feat.version, "1.0");
        assert_eq!(feat.description, "On-demand FFmpeg audio transcoding");
        assert!(
            feat.enabled,
            "transcode must be enabled when ffmpeg is available and stream is enabled"
        );
        assert_eq!(
            feat.maturity,
            FeatureMaturity::Stable,
            "maturity must be Stable per product truth"
        );
        assert_eq!(feat.evidence, EvidenceLevel::IntegrationCertified);
    }

    #[test]
    fn test_transcode_no_duplicate_aliases() {
        let disabled = HashSet::new();
        let caps = ServerCapabilities::from_parts(&disabled, 0, true);

        assert!(
            caps.features.iter().all(|f| f.name != "transcoding"),
            "features must NOT contain duplicate or legacy alias 'transcoding'"
        );
        let transcode_count = caps
            .features
            .iter()
            .filter(|f| f.name == "transcode")
            .count();
        assert_eq!(
            transcode_count, 1,
            "exactly one canonical 'transcode' feature allowed"
        );
    }

    #[test]
    fn test_transcode_canonical_feature_enabled_lookup() {
        let disabled = HashSet::new();
        let caps = ServerCapabilities::from_parts(&disabled, 0, true);

        assert!(
            caps.feature_enabled("transcode"),
            "feature_enabled('transcode') must return true when ffmpeg is available"
        );
        assert!(
            !caps.feature_enabled("transcoding"),
            "feature_enabled('transcoding') must return false as 'transcoding' is not a runtime feature"
        );
    }

    #[test]
    fn test_transcode_negative_availability() {
        // Case 1: ffmpeg unavailable
        let disabled = HashSet::new();
        let caps_no_ffmpeg = ServerCapabilities::from_parts(&disabled, 0, false);
        assert!(
            !caps_no_ffmpeg.feature_enabled("transcode"),
            "transcode must be disabled when ffmpeg is unavailable"
        );
        let feat = caps_no_ffmpeg
            .features
            .iter()
            .find(|f| f.name == "transcode")
            .unwrap();
        assert!(!feat.enabled);
        assert_eq!(
            feat.maturity,
            FeatureMaturity::Stable,
            "maturity remains Stable even if unavailable"
        );

        // Case 2: stream module disabled
        let mut disabled_stream = HashSet::new();
        disabled_stream.insert("stream".to_string());
        let caps_no_stream = ServerCapabilities::from_parts(&disabled_stream, 0, true);
        assert!(
            !caps_no_stream.feature_enabled("transcode"),
            "transcode must be disabled when stream module is disabled"
        );
        let feat_stream = caps_no_stream
            .features
            .iter()
            .find(|f| f.name == "transcode")
            .unwrap();
        assert!(!feat_stream.enabled);

        // Case 3: both ffmpeg unavailable and stream disabled
        let caps_neither = ServerCapabilities::from_parts(&disabled_stream, 0, false);
        assert!(!caps_neither.feature_enabled("transcode"));
    }

    #[test]
    fn test_product_truth_runtime_projection_consistency() {
        let disabled = HashSet::new();
        let caps = ServerCapabilities::from_parts(&disabled, 0, true);

        // Invariant: canonical 'transcode' declared in CANONICAL_MATURITY must be materialized as a runtime ServerFeature
        let has_canonical_transcode = CANONICAL_MATURITY
            .iter()
            .any(|(name, _)| *name == "transcode");
        assert!(
            has_canonical_transcode,
            "'transcode' must exist in CANONICAL_MATURITY"
        );

        let runtime_transcode = caps.features.iter().find(|f| f.name == "transcode");
        assert!(
            runtime_transcode.is_some(),
            "canonical 'transcode' in Product Truth must have a runtime ServerFeature projection"
        );
        assert_eq!(
            runtime_transcode.unwrap().maturity,
            FeatureMaturity::Stable,
            "runtime transcode projection must inherit canonical maturity"
        );
    }
}
