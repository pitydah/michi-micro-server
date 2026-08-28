use async_trait::async_trait;
use michi_playback::{AudioSink, PcmFormat, PlaybackError, SinkKind, SinkSnapshot, SinkState};
use michi_receivers::ReceiverSessionManager;
use tracing::{debug, warn};
use uuid::Uuid;

pub struct ReceiverAudioSink {
    receiver_id: String,
    session_manager: ReceiverSessionManager,
    state: SinkState,
    bytes_received: u64,
    bytes_sent_to_transport: u64,
    last_error: Option<String>,
    volume: u8,
    muted: bool,
}

impl ReceiverAudioSink {
    pub fn new(receiver_id: String, session_manager: ReceiverSessionManager) -> Self {
        Self {
            receiver_id,
            session_manager,
            state: SinkState::Preparing,
            bytes_received: 0,
            bytes_sent_to_transport: 0,
            last_error: None,
            volume: 80,
            muted: false,
        }
    }

    pub fn new_with_config(
        receiver_id: String,
        session_manager: ReceiverSessionManager,
        volume: u8,
        muted: bool,
    ) -> Self {
        Self {
            receiver_id,
            session_manager,
            state: SinkState::Preparing,
            bytes_received: 0,
            bytes_sent_to_transport: 0,
            last_error: None,
            volume,
            muted,
        }
    }
}

#[async_trait]
impl AudioSink for ReceiverAudioSink {
    fn id(&self) -> &str {
        &self.receiver_id
    }

    fn kind(&self) -> SinkKind {
        SinkKind::Receiver
    }

    async fn prepare(&mut self, format: PcmFormat) -> Result<(), PlaybackError> {
        let registry_arc = self.session_manager.registry().await;
        let registry = registry_arc.read().await;
        let entry = registry
            .get(&self.receiver_id)
            .ok_or_else(|| PlaybackError::ReceiverNotPaired(self.receiver_id.clone()))?;

        if !entry.paired {
            self.state = SinkState::Failed;
            self.last_error = Some("receiver is not paired".to_string());
            return Err(PlaybackError::ReceiverNotPaired(self.receiver_id.clone()));
        }
        drop(registry);

        // Check if session is already active; if not, start session
        let active = self
            .session_manager
            .get_active_session(&self.receiver_id)
            .await;

        if active.is_none() {
            let session_id = Uuid::new_v4().to_string();
            match self
                .session_manager
                .start_session(
                    &self.receiver_id,
                    &session_id,
                    "pcm_s16le",
                    format.sample_rate,
                    format.bit_depth as u32,
                    format.channels as u32,
                    0,
                    200,
                    self.volume as u32,
                )
                .await
            {
                Ok(_) => {
                    debug!("started receiver session for {}", self.receiver_id);
                }
                Err(e) => {
                    self.state = SinkState::Failed;
                    self.last_error = Some(e.clone());
                    return Err(PlaybackError::PlaybackFailed(format!(
                        "failed to start receiver session for {}: {}",
                        self.receiver_id, e
                    )));
                }
            }
        }

        self.state = SinkState::Ready;
        self.last_error = None;
        Ok(())
    }

    async fn write_pcm(&mut self, data: &[u8]) -> Result<usize, PlaybackError> {
        self.bytes_received += data.len() as u64;

        if self.muted {
            self.state = SinkState::AudioFlowing;
            return Ok(0); // 0 bytes sent to network transport when muted
        }

        match self
            .session_manager
            .write_pcm(&self.receiver_id, data)
            .await
        {
            Ok(written) => {
                self.bytes_sent_to_transport += written as u64;
                self.state = SinkState::AudioFlowing;
                self.last_error = None;
                Ok(written)
            }
            Err(e) => {
                warn!("receiver {} write_pcm error: {}", self.receiver_id, e);
                self.state = SinkState::Failed;
                self.last_error = Some(e.clone());
                Err(PlaybackError::PlaybackFailed(e))
            }
        }
    }

    async fn pause(&mut self) -> Result<(), PlaybackError> {
        self.state = SinkState::Paused;
        Ok(())
    }

    async fn resume(&mut self) -> Result<(), PlaybackError> {
        self.state = SinkState::AudioFlowing;
        Ok(())
    }

    async fn set_volume(&mut self, volume: u8) -> Result<(), PlaybackError> {
        self.volume = volume;
        let _ = self
            .session_manager
            .set_volume(&self.receiver_id, volume as u32)
            .await;
        Ok(())
    }

    async fn health(&self) -> Result<(), PlaybackError> {
        let registry_arc = self.session_manager.registry().await;
        let registry = registry_arc.read().await;
        if let Some(entry) = registry.get(&self.receiver_id) {
            if entry.paired {
                Ok(())
            } else {
                Err(PlaybackError::ReceiverNotPaired(self.receiver_id.clone()))
            }
        } else {
            Err(PlaybackError::ReceiverNotPaired(self.receiver_id.clone()))
        }
    }

    async fn stop(&mut self) -> Result<(), PlaybackError> {
        let _ = self.session_manager.stop_session(&self.receiver_id).await;
        self.state = SinkState::Stopped;
        Ok(())
    }

    fn snapshot(&self) -> SinkSnapshot {
        SinkSnapshot {
            sink_id: self.receiver_id.clone(),
            kind: SinkKind::Receiver,
            state: self.state,
            bytes_received: self.bytes_received,
            bytes_sent_to_transport: self.bytes_sent_to_transport,
            muted: self.muted,
            last_error: self.last_error.clone(),
        }
    }
}
