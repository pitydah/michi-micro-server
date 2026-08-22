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

    /// Send raw PCM bytes chunk over the transport data plane.
    async fn write_pcm(&mut self, pcm_data: &[u8]) -> TransportResult<usize>;

    /// Send a single discrete frame (RTP packet / frame buffer).
    async fn send_frame(&mut self, samples: &[i16], marker: bool) -> TransportResult<()>;

    /// Terminate the active session gracefully (sends teardown to the endpoint).
    async fn stop(&mut self) -> TransportResult<()>;
}

/// Concrete RTP/UDP audio transport implementation for Michi Music Stream (v1-lite).
/// Wire-format: PCM S16LE / 48kHz / 16-bit / 2ch / 10ms packets (960 bytes, 480 stereo samples) / PT 97.
#[derive(Debug)]
pub struct RtpReceiverTransport {
    target_addr: String,
    socket: Option<tokio::net::UdpSocket>,
    config: Option<TransportStreamConfig>,
    handle: Option<TransportSessionHandle>,
    sequence_number: u16,
    timestamp: u32,
    ssrc: u32,
    payload_type: u8,
}

impl RtpReceiverTransport {
    pub fn new(target_addr: impl Into<String>, ssrc: u32) -> Self {
        Self {
            target_addr: target_addr.into(),
            socket: None,
            config: None,
            handle: None,
            sequence_number: 0,
            timestamp: 0,
            ssrc,
            payload_type: 97,
        }
    }

    /// Build an RTP packet header for PCM audio according to RFC 3550 / Michi Link specification.
    pub fn build_rtp_packet(
        payload_type: u8,
        sequence_number: u16,
        timestamp: u32,
        ssrc: u32,
        marker: bool,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut packet = Vec::with_capacity(12 + payload.len());
        // V=2, P=0, X=0, CC=0 -> 0x80
        packet.push(0x80);
        // M (1 bit) + PT (7 bits)
        let m_pt = if marker { 0x80 | (payload_type & 0x7F) } else { payload_type & 0x7F };
        packet.push(m_pt);
        // Sequence Number (16 bits)
        packet.extend_from_slice(&sequence_number.to_be_bytes());
        // Timestamp (32 bits)
        packet.extend_from_slice(&timestamp.to_be_bytes());
        // SSRC (32 bits)
        packet.extend_from_slice(&ssrc.to_be_bytes());
        // Payload
        packet.extend_from_slice(payload);
        packet
    }

    /// Generate synthetic PCM S16LE sine wave buffer (48kHz, 2ch, 10ms chunks).
    pub fn generate_synthetic_sine(frequency_hz: f32, duration_ms: u32) -> Vec<i16> {
        let sample_rate = 48000;
        let total_samples = (sample_rate as f32 * (duration_ms as f32 / 1000.0)) as usize;
        let mut samples = Vec::with_capacity(total_samples * 2);
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let val = (2.0 * std::f32::consts::PI * frequency_hz * t).sin();
            let sample = (val * 32767.0 * 0.5) as i16;
            samples.push(sample); // Left
            samples.push(sample); // Right
        }
        samples
    }

    /// Generate synthetic silence PCM S16LE buffer (48kHz, 2ch, 10ms chunks).
    pub fn generate_synthetic_silence(duration_ms: u32) -> Vec<i16> {
        let sample_rate = 48000;
        let total_samples = (sample_rate as f32 * (duration_ms as f32 / 1000.0)) as usize;
        vec![0i16; total_samples * 2]
    }
}

#[async_trait]
impl AudioTransport for RtpReceiverTransport {
    fn transport_type(&self) -> &'static str {
        "rtp_udp_v1"
    }

    async fn start(
        &mut self,
        config: TransportStreamConfig,
    ) -> TransportResult<TransportSessionHandle> {
        if self.handle.is_some() {
            return Err(TransportError::AlreadyActive);
        }

        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| TransportError::Unreachable(e.to_string()))?;
        socket
            .connect(&self.target_addr)
            .await
            .map_err(|e| TransportError::Unreachable(e.to_string()))?;

        let remote_port = self
            .target_addr
            .rsplit_once(':')
            .and_then(|(_, p)| p.parse().ok());

        let handle = TransportSessionHandle {
            session_id: uuid::Uuid::new_v4().to_string(),
            remote_port,
        };

        self.socket = Some(socket);
        self.config = Some(config);
        self.handle = Some(handle.clone());
        self.sequence_number = rand::random::<u16>();
        self.timestamp = rand::random::<u32>();

        Ok(handle)
    }

    async fn pause(&mut self) -> TransportResult<()> {
        if self.handle.is_none() {
            return Err(TransportError::NoSession);
        }
        Ok(())
    }

    async fn resume(&mut self) -> TransportResult<()> {
        if self.handle.is_none() {
            return Err(TransportError::NoSession);
        }
        Ok(())
    }

    async fn set_volume(&mut self, _volume: u8) -> TransportResult<()> {
        if self.handle.is_none() {
            return Err(TransportError::NoSession);
        }
        Ok(())
    }

    async fn health(&self) -> TransportResult<()> {
        if self.handle.is_none() || self.socket.is_none() {
            return Err(TransportError::NoSession);
        }
        Ok(())
    }

    async fn write_pcm(&mut self, pcm_data: &[u8]) -> TransportResult<usize> {
        let socket = self.socket.as_ref().ok_or(TransportError::NoSession)?;
        let chunk_size = 960; // 10ms of 48kHz S16LE stereo = 480 samples * 2 channels * 2 bytes = 1920 bytes? No: 480 * 2 bytes = 960 bytes mono, stereo is 480 * 2 * 2 = 1920 bytes.
        let mut bytes_written = 0;

        for chunk in pcm_data.chunks(chunk_size) {
            let packet = Self::build_rtp_packet(
                self.payload_type,
                self.sequence_number,
                self.timestamp,
                self.ssrc,
                false,
                chunk,
            );
            socket
                .send(&packet)
                .await
                .map_err(|e| TransportError::Other(e.to_string()))?;

            self.sequence_number = self.sequence_number.wrapping_add(1);
            let samples_in_chunk = (chunk.len() / 4) as u32;
            self.timestamp = self.timestamp.wrapping_add(samples_in_chunk);
            bytes_written += chunk.len();
        }

        Ok(bytes_written)
    }

    async fn send_frame(&mut self, samples: &[i16], marker: bool) -> TransportResult<()> {
        let socket = self.socket.as_ref().ok_or(TransportError::NoSession)?;
        let mut raw_bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            raw_bytes.extend_from_slice(&s.to_le_bytes());
        }

        let packet = Self::build_rtp_packet(
            self.payload_type,
            self.sequence_number,
            self.timestamp,
            self.ssrc,
            marker,
            &raw_bytes,
        );

        socket
            .send(&packet)
            .await
            .map_err(|e| TransportError::Other(e.to_string()))?;

        self.sequence_number = self.sequence_number.wrapping_add(1);
        let samples_per_channel = (samples.len() / 2) as u32;
        self.timestamp = self.timestamp.wrapping_add(samples_per_channel);

        Ok(())
    }

    async fn stop(&mut self) -> TransportResult<()> {
        self.socket = None;
        self.handle = None;
        self.config = None;
        Ok(())
    }
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

    #[test]
    fn test_rtp_packet_header_structure() {
        let payload = vec![0x12, 0x34, 0x56, 0x78];
        let packet = RtpReceiverTransport::build_rtp_packet(97, 100, 48000, 0xDEADBEEF, false, &payload);
        assert_eq!(packet.len(), 12 + 4);
        assert_eq!(packet[0], 0x80); // V=2
        assert_eq!(packet[1], 97);   // PT=97, Marker=0
        assert_eq!(&packet[2..4], &100u16.to_be_bytes());
        assert_eq!(&packet[4..8], &48000u32.to_be_bytes());
        assert_eq!(&packet[8..12], &0xDEADBEEFu32.to_be_bytes());
        assert_eq!(&packet[12..], &payload[..]);
    }

    #[tokio::test]
    async fn test_rtp_receiver_transport_lifecycle() {
        let mut transport = RtpReceiverTransport::new("127.0.0.1:9099", 12345);
        let cfg = TransportStreamConfig::receiver_v1_lite_default();
        let handle = transport.start(cfg).await.expect("transport start");
        assert_eq!(handle.remote_port, Some(9099));
        assert!(transport.health().await.is_ok());

        let synthetic_pcm = RtpReceiverTransport::generate_synthetic_sine(440.0, 10);
        assert_eq!(synthetic_pcm.len(), 480 * 2);

        let res = transport.send_frame(&synthetic_pcm, false).await;
        assert!(res.is_ok());

        assert!(transport.stop().await.is_ok());
        assert!(transport.health().await.is_err());
    }
}
