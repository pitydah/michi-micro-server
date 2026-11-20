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
    /// Single authoritative canonical capability projection from `michi-identity`.
    #[serde(skip)]
    pub canonical: michi_identity::CanonicalServerCapabilities,
    /// Derived runtime transcoding availability (ffmpeg + stream/transcode modules active).
    #[serde(skip)]
    pub transcoding_ready: bool,
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
    ("pawpass_v1", FeatureMaturity::Beta),
    ("perch_client_v1", FeatureMaturity::Beta),
    ("playback_history", FeatureMaturity::Stable),
    ("playlists", FeatureMaturity::Stable),
    ("receivers", FeatureMaturity::Beta),
    ("rooms", FeatureMaturity::Beta),
    ("search", FeatureMaturity::Stable),
    ("security", FeatureMaturity::Stable),
    ("stream", FeatureMaturity::Stable),
    ("sync", FeatureMaturity::Stable),
    ("tailsync_v1", FeatureMaturity::Beta),
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
        "pawpass_v1",
        "1.0",
        "PawPass distributed playback transfer target and source",
        FeatureMaturity::Beta,
        EvidenceLevel::Implemented,
    ),
    (
        "perch_client_v1",
        "1.0",
        "Perch authority client, gate and takeover",
        FeatureMaturity::Beta,
        EvidenceLevel::EffectVerified,
    ),
    (
        "tailsync_v1",
        "1.0",
        "TailSync playback state synchronization",
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
            .iter()
            .filter(|r| r.presence == michi_receivers::ReceiverPresence::VerifiedOnline)
            .count();
        let ffmpeg = michi_streaming::check_ffmpeg();
        let tls_ready = state.is_aircat_tls_ready().await;

        let library_enabled = !disabled.contains("library");
        let search_enabled = !disabled.contains("search") && library_enabled;
        let streaming_enabled = !disabled.contains("stream") && !disabled.contains("streaming");
        let download_enabled = !disabled.contains("download") && library_enabled;
        let sync_enabled = !disabled.contains("sync");
        let sync_manifest_enabled = sync_enabled && library_enabled;
        let artwork_enabled = !disabled.contains("artwork") && library_enabled;
        let playlists_enabled = !disabled.contains("playlists") && library_enabled;
        let playback_enabled = !disabled.contains("playback");
        let queue_enabled = playback_enabled && !disabled.contains("queue");
        let events_enabled = !disabled.contains("events");
        let receivers_enabled = !disabled.contains("receivers");
        let rooms_enabled = !disabled.contains("rooms");
        let multiroom_enabled = rooms_enabled && receivers_enabled;
        let transcoding_ready = ffmpeg
            && streaming_enabled
            && !disabled.contains("transcoding")
            && !disabled.contains("transcode");

        // Single authoritative runtime capability projection governed by michi-link
        let canonical = michi_identity::CanonicalServerCapabilities::from_runtime_state(
            &michi_identity::RuntimeCapabilityState {
                library: library_enabled,
                search: search_enabled,
                streaming: streaming_enabled,
                download: download_enabled,
                sync: sync_enabled,
                sync_manifest: sync_manifest_enabled,
                artwork: artwork_enabled,
                playlists: playlists_enabled,
                playback: playback_enabled,
                queue: queue_enabled,
                events: events_enabled,
                receivers: receivers_enabled,
                multiroom: multiroom_enabled,
                aircat_tls_listener_ready: tls_ready,
                aircat_producer_ready: tls_ready && library_enabled,
            },
        );
        let canonical_map = canonical.to_features_map();

        let mut features: Vec<ServerFeature> = MODULE_FEATURES
            .iter()
            .map(|(name, version, description, maturity, evidence)| {
                let enabled = match *name {
                    "stream" => canonical.streaming,
                    "sync" => canonical.sync,
                    "playback" => canonical.playback,
                    "receivers" => canonical.receivers,
                    "rooms" => rooms_enabled,
                    other => !disabled.contains(other),
                };
                ServerFeature {
                    name,
                    version,
                    description,
                    enabled,
                    maturity: canonical_maturity(name, *maturity),
                    evidence: *evidence,
                }
            })
            .collect();

        features.extend(ALWAYS_ON_FEATURES.iter().map(
            |(name, version, description, maturity, evidence)| {
                let enabled = if let Some(&canon_val) = canonical_map.get(*name) {
                    canon_val
                } else if *name == "autonomous_playback" {
                    ffmpeg && canonical.playback
                } else if *name == "import" {
                    library_enabled && !disabled.contains("import")
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

        // Canonical Michi Link capabilities not yet in legacy MODULE/ALWAYS_ON tables
        features.push(ServerFeature {
            name: michi_identity::CAP_STREAMING,
            version: "1.0",
            description: "Canonical HTTP audio streaming capability (aliased by legacy 'stream')",
            enabled: canonical.streaming,
            maturity: canonical_maturity("stream", FeatureMaturity::Stable),
            evidence: EvidenceLevel::EffectVerified,
        });
        features.push(ServerFeature {
            name: michi_identity::CAP_SYNC_MANIFEST,
            version: "1.0",
            description: "Library delta & manifest synchronization endpoints",
            enabled: canonical.sync_manifest,
            maturity: canonical_maturity("sync", FeatureMaturity::Stable),
            evidence: EvidenceLevel::EffectVerified,
        });
        features.push(ServerFeature {
            name: michi_identity::CAP_MULTIROOM,
            version: "1.0",
            description: "Multiroom groups & synchronized receiver playback",
            enabled: canonical.multiroom,
            maturity: canonical_maturity("rooms", FeatureMaturity::Beta),
            evidence: EvidenceLevel::EffectVerified,
        });
        features.push(ServerFeature {
            name: "transcoding",
            version: "1.0",
            description: "On-the-fly audio transcoding (requires FFmpeg and active stream module)",
            enabled: transcoding_ready,
            maturity: canonical_maturity("transcode", FeatureMaturity::Stable),
            evidence: if transcoding_ready {
                EvidenceLevel::EffectVerified
            } else {
                EvidenceLevel::Implemented
            },
        });

        let aircat_evidence = if canonical.aircat_original_v1 {
            EvidenceLevel::EffectVerified
        } else {
            EvidenceLevel::Implemented
        };
        features.push(ServerFeature {
            name: michi_identity::CAP_AIRCAT_ORIGINAL_V1,
            version: "1.0",
            description: "AirCat v1 original file transfer (requires active TLS/SPKI and library)",
            enabled: canonical.aircat_original_v1,
            maturity: FeatureMaturity::Beta,
            evidence: aircat_evidence,
        });
        features.push(ServerFeature {
            name: michi_identity::CAP_HOME_TLS_SPKI_PINNED,
            version: "1.0",
            description: "RFC 7469 TLS SPKI pin active on HTTPS listener",
            enabled: canonical.home_tls_spki_pinned,
            maturity: FeatureMaturity::Beta,
            evidence: if canonical.home_tls_spki_pinned {
                EvidenceLevel::EffectVerified
            } else {
                EvidenceLevel::Implemented
            },
        });
        features.push(ServerFeature {
            name: michi_identity::CAP_AIRCAT_MANIFEST_V1,
            version: "1.0",
            description: "AirCat v1 original catalog manifest over pinned HTTPS",
            enabled: canonical.aircat_manifest_v1,
            maturity: FeatureMaturity::Beta,
            evidence: aircat_evidence,
        });
        features.push(ServerFeature {
            name: michi_identity::CAP_AIRCAT_AUTHORIZE_V1,
            version: "1.0",
            description: "AirCat v1 Ed25519 grant authorization over pinned HTTPS",
            enabled: canonical.aircat_authorize_v1,
            maturity: FeatureMaturity::Beta,
            evidence: aircat_evidence,
        });
        features.push(ServerFeature {
            name: michi_identity::CAP_AIRCAT_ORIGINAL_DOWNLOAD_V1,
            version: "1.0",
            description: "AirCat v1 original-bytes download with Range/ETag over pinned HTTPS",
            enabled: canonical.aircat_original_download_v1,
            maturity: FeatureMaturity::Beta,
            evidence: aircat_evidence,
        });

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
                ServerProtocol {
                    name: "aircat",
                    version: "1.0",
                },
            ],
            runtime: ServerRuntime {
                receivers_connected: receiver_count,
                ffmpeg_available: ffmpeg,
            },
            canonical,
            transcoding_ready,
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
        // Guarantee canonical keys are always present with identical values
        for (k, v) in self.canonical.to_features_map() {
            map.insert(k, serde_json::Value::Bool(v));
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
        let mut map = self.canonical.to_discovery_features();
        // Legacy discovery alias derived strictly from canonical streaming flag
        map.insert("stream".to_string(), self.canonical.streaming);
        map
    }
}
