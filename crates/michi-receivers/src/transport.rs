//! Audio transport abstraction — M4.
//!
//! Decouples [`PlaybackSession`] from any concrete transport protocol.
//!
//! # Design intent
//!
//! Currently the only implementation is RTP/UDP (receiver-v1-lite).
//! The trait ensures that, in the future, additional transports (Sendspin,
//! Snapcast, local ALSA) can be plugged in **without rewriting session,
//! zone, or receiver-manager code**.
//!
//! # Wire-format contract
//!
//! `AudioTransport` does NOT define the wire format. The wire format for
//! receiver-v1-lite (`PCM S16LE / 48 kHz / 16-bit / 2ch / PT 97 / 10 ms`)
//! is unchanged and remains in [`crate::client::ReceiverClient`].
//!
//! # Dependency direction
//!
//! ```text
//! PlaybackSession  →  AudioTransport  ←  RtpReceiverTransport
//!                                      ↑
//!                              (future: SnapcastTransport, AlsaTransport …)
//! ```

use async_trait::async_trait;

/// Session-level description of the audio stream being transported.
#[derive(Debug, Clone)]
pub struct TransportStreamConfig {
    /// Codec identifier, e.g. `"pcm_s16le"`.
    pub codec: String,
    /// Sample rate in Hz (e.g. 48_000).
    pub sample_rate: u32,
    /// Bit depth (e.g. 16).
    pub bit_depth: u32,
    /// Channel count (e.g. 2).
    pub channels: u8,
    /// Packet duration in milliseconds (relevant for RTP; 10 ms = receiver-v1-lite default).
    pub packet_ms: u32,
}

impl TransportStreamConfig {
    /// Returns the certified default configuration for Michi Stream receiver-v1-lite.
    ///
    /// This is the ONLY wire format certified by physical hardware tests as of
    /// the current release. Do NOT change these values without device-level evidence.
    pub fn receiver_v1_lite_default() -> Self {
        Self {
            codec: "pcm_s16le".into(),
            sample_rate: 48_000,
            bit_depth: 16,
            channels: 2,
            packet_ms: 10,
        }
    }
}

/// Opaque handle returned by a successful [`AudioTransport::start`].
/// Transports use this to route teardown, pause, and resume to the right session.
#[derive(Debug, Clone)]
pub struct TransportSessionHandle {
    /// Unique ID for this transport session (may differ from playback session ID).
    pub session_id: String,
    /// The port on the remote endpoint that is receiving audio packets.
    pub remote_port: Option<u16>,
}

/// Result type for transport operations.
pub type TransportResult<T> = Result<T, TransportError>;

/// Errors that a transport implementation may return.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("transport session already active")]
    AlreadyActive,
    #[error("no active transport session")]
    NoSession,
    #[error("endpoint unreachable: {0}")]
    Unreachable(String),
    #[error("capability mismatch: {0}")]
    CapabilityMismatch(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("transport error: {0}")]
    Other(String),
}

/// Hardware-agnostic audio transport layer.
///
/// Implementations are responsible for **one concern only**: delivering audio
/// data from Michi Server to a specific audio sink. They must NOT:
/// - contain library logic;
/// - contain UI logic;
/// - manage playback queue or shuffle state.
///
/// # Lifecycle
///
/// ```text
/// create()  →  start()  →  [write() …]  →  pause() / resume()  →  stop()  →  close()
///                                ↑ health() may be called any time after start()
/// ```
#[async_trait]
pub trait AudioTransport: Send + Sync {
    /// Identifier string for this transport type (e.g. `"rtp_udp_v1"`).
    fn transport_type(&self) -> &'static str;

    /// Negotiate and open a transport session to the audio endpoint.
    ///
    /// Returns a [`TransportSessionHandle`] that callers must pass to subsequent operations.
    /// Errors if a session is already active on this transport instance.
    async fn start(
        &mut self,
        config: TransportStreamConfig,
    ) -> TransportResult<TransportSessionHandle>;

    /// Pause audio delivery without tearing down the session.
    async fn pause(&mut self) -> TransportResult<()>;

    /// Resume a previously paused session.
    async fn resume(&mut self) -> TransportResult<()>;

    /// Set the output volume (0–100).
    async fn set_volume(&mut self, volume: u8) -> TransportResult<()>;

    /// Probe the health of an active session.
    ///
    /// Returns `Ok(())` if the session is alive, or an error describing the failure.
    async fn health(&self) -> TransportResult<()>;

    /// Terminate the active session gracefully (sends teardown to the endpoint).
    async fn stop(&mut self) -> TransportResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_v1_lite_default_values_are_canonical() {
        let cfg = TransportStreamConfig::receiver_v1_lite_default();
        assert_eq!(cfg.codec, "pcm_s16le");
        assert_eq!(cfg.sample_rate, 48_000);
        assert_eq!(cfg.bit_depth, 16);
        assert_eq!(cfg.channels, 2);
        assert_eq!(cfg.packet_ms, 10);
    }

    #[test]
    fn transport_error_display() {
        let e = TransportError::Unreachable("10.0.0.1:9000".into());
        assert!(e.to_string().contains("unreachable"));
    }
}
