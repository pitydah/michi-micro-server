use chrono::{DateTime, Utc};
use michi_core::Track;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::error::PlaybackError;
use crate::sink::AudioSink;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackLifecycle {
    #[default]
    Idle,
    Preparing,
    Buffering,
    AudioFlowing,
    Playing,
    Paused,
    Stopping,
    Stopped,
    Ended,
    Failed,
}

impl PlaybackLifecycle {
    pub fn is_playing(&self) -> bool {
        matches!(self, Self::AudioFlowing | Self::Playing)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Preparing => "preparing",
            Self::Buffering => "buffering",
            Self::AudioFlowing => "audio_flowing",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Ended => "ended",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }
}

impl std::str::FromStr for RepeatMode {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "all" => Ok(Self::All),
            "one" | "track" => Ok(Self::One),
            _ => Ok(Self::Off),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u16,
}

impl Default for PcmFormat {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            bit_depth: 16,
        }
    }
}

impl PcmFormat {
    pub fn bytes_per_frame(&self) -> usize {
        (self.channels as usize) * (self.bit_depth as usize / 8)
    }

    pub fn bytes_for_duration_ms(&self, ms: u64) -> usize {
        ((self.sample_rate as u64 * ms * self.bytes_per_frame() as u64) / 1000) as usize
    }

    pub fn duration_ms_for_bytes(&self, bytes: usize) -> u64 {
        let bpf = self.bytes_per_frame();
        if bpf == 0 || self.sample_rate == 0 {
            return 0;
        }
        (bytes as u64 * 1000) / (self.sample_rate as u64 * bpf as u64)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SinkKind {
    Receiver,
    RoomGroup,
    Chain,
    Local,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SinkState {
    Preparing,
    Ready,
    AudioFlowing,
    Paused,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SinkSnapshot {
    pub sink_id: String,
    pub kind: SinkKind,
    pub state: SinkState,
    pub bytes_received: u64,
    pub bytes_sent_to_transport: u64,
    pub muted: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaybackOutputDescription {
    pub target_id: String,
    pub target_name: String,
    pub kind: String,
    pub receiver_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EngineSnapshot {
    pub lifecycle: PlaybackLifecycle,
    pub generation_id: u64,
    pub track_id: Option<Uuid>,
    pub current_track: Option<Track>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub output: Option<PlaybackOutputDescription>,
    pub sinks: Vec<SinkSnapshot>,
    pub failed_sinks: Vec<SinkSnapshot>,
    pub track_bytes_decoded: u64,
    pub track_pcm_timeline_bytes: u64,
    pub network_bytes_sent_total: u64,
    pub bytes_decoded: u64,
    pub bytes_delivered: u64,
    pub output_health: String,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for EngineSnapshot {
    fn default() -> Self {
        Self {
            lifecycle: PlaybackLifecycle::Idle,
            generation_id: 0,
            track_id: None,
            current_track: None,
            position_ms: 0,
            duration_ms: None,
            volume: 80,
            shuffle: false,
            repeat: RepeatMode::Off,
            output: None,
            sinks: Vec::new(),
            failed_sinks: Vec::new(),
            track_bytes_decoded: 0,
            track_pcm_timeline_bytes: 0,
            network_bytes_sent_total: 0,
            bytes_decoded: 0,
            bytes_delivered: 0,
            output_health: "none".to_string(),
            last_error: None,
            updated_at: Utc::now(),
        }
    }
}

impl EngineSnapshot {
    pub fn is_playing(&self) -> bool {
        self.lifecycle.is_playing()
    }
}

pub enum EngineCommand {
    Play {
        track: Box<Track>,
        sinks: Vec<Box<dyn AudioSink>>,
        output_desc: PlaybackOutputDescription,
        position_ms: u64,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    LoadTrack {
        track: Box<Track>,
        position_ms: u64,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    JumpToIndex {
        index: usize,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    Pause {
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    Resume {
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    Seek {
        position_ms: u64,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    Next {
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    Previous {
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    Stop {
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    SetVolume {
        volume: u8,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    SetShuffle {
        shuffle: bool,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    SetRepeat {
        repeat: RepeatMode,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    SetQueue {
        tracks: Vec<Track>,
        current_index: usize,
        current_track_id: Option<Uuid>,
        respond_to: oneshot::Sender<Result<(), PlaybackError>>,
    },
    GetSnapshot {
        respond_to: oneshot::Sender<EngineSnapshot>,
    },
    Shutdown {
        respond_to: oneshot::Sender<()>,
    },
}
