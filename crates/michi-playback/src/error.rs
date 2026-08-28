use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("NO_OUTPUT_SELECTED: no playback output target selected")]
    NoOutputSelected,

    #[error("OUTPUT_NOT_FOUND: output '{0}' was not found")]
    OutputNotFound(String),

    #[error("OUTPUT_UNAVAILABLE: output '{0}' is unavailable")]
    OutputUnavailable(String),

    #[error("TRACK_NOT_FOUND: track '{0}' was not found")]
    TrackNotFound(Uuid),

    #[error("TRACK_FILE_MISSING: track audio file '{0}' does not exist on disk")]
    TrackFileMissing(String),

    #[error("DECODER_UNAVAILABLE: ffmpeg decoder is not available: {0}")]
    DecoderUnavailable(String),

    #[error("DECODER_FAILED: decoding failed: {0}")]
    DecoderFailed(String),

    #[error("INVALID_MEDIA: invalid media file: {0}")]
    InvalidMedia(String),

    #[error("PLAYBACK_FAILED: playback failed: {0}")]
    PlaybackFailed(String),

    #[error("ALL_OUTPUTS_FAILED: all output sinks failed to render audio")]
    AllOutputsFailed,

    #[error("RECEIVER_NOT_PAIRED: receiver '{0}' is not paired")]
    ReceiverNotPaired(String),

    #[error("RECEIVER_OFFLINE: receiver '{0}' is offline")]
    ReceiverOffline(String),

    #[error("INVALID_SEEK: requested seek position {0}ms is invalid")]
    InvalidSeek(u64),

    #[error("INVALID_VOLUME: volume {0} is out of valid range 0..=100")]
    InvalidVolume(u8),

    #[error("TRACK_OUTSIDE_LIBRARY: track file '{0}' is outside permitted music library paths")]
    TrackOutsideLibrary(String),

    #[error("QUEUE_EMPTY: playback queue is empty")]
    QueueEmpty,

    #[error("QUEUE_INDEX_INVALID: queue index {0} is out of bounds")]
    QueueIndexInvalid(usize),

    #[error("NO_TRACK_SELECTED: no track was selected for playback")]
    NoTrackSelected,

    #[error("ENGINE_UNAVAILABLE: playback engine is unavailable")]
    EngineUnavailable,

    #[error("CHANNEL_CLOSED: playback engine command channel closed")]
    ChannelClosed,

    #[error("IO_ERROR: {0}")]
    Io(#[from] std::io::Error),

    #[error("DATABASE_ERROR: {0}")]
    Database(#[from] michi_db::DbError),
}

impl PlaybackError {
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::NoOutputSelected => "NO_OUTPUT_SELECTED",
            Self::OutputNotFound(_) => "OUTPUT_NOT_FOUND",
            Self::OutputUnavailable(_) => "OUTPUT_UNAVAILABLE",
            Self::TrackNotFound(_) => "TRACK_NOT_FOUND",
            Self::TrackFileMissing(_) => "TRACK_FILE_MISSING",
            Self::TrackOutsideLibrary(_) => "TRACK_OUTSIDE_LIBRARY",
            Self::QueueEmpty => "QUEUE_EMPTY",
            Self::QueueIndexInvalid(_) => "QUEUE_INDEX_INVALID",
            Self::NoTrackSelected => "NO_TRACK_SELECTED",
            Self::EngineUnavailable => "ENGINE_UNAVAILABLE",
            Self::DecoderUnavailable(_) => "DECODER_UNAVAILABLE",
            Self::DecoderFailed(_) => "DECODER_FAILED",
            Self::InvalidMedia(_) => "INVALID_MEDIA",
            Self::PlaybackFailed(_) => "PLAYBACK_FAILED",
            Self::AllOutputsFailed => "ALL_OUTPUTS_FAILED",
            Self::ReceiverNotPaired(_) => "RECEIVER_NOT_PAIRED",
            Self::ReceiverOffline(_) => "RECEIVER_OFFLINE",
            Self::InvalidSeek(_) => "INVALID_SEEK",
            Self::InvalidVolume(_) => "INVALID_VOLUME",
            Self::ChannelClosed => "CHANNEL_CLOSED",
            Self::Io(_) => "IO_ERROR",
            Self::Database(_) => "DATABASE_ERROR",
        }
    }
}
