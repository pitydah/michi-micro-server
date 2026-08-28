use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use michi_core::Track;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::decoder::FfmpegPcmDecoder;
use crate::error::PlaybackError;
use crate::model::{
    EngineCommand, EngineSnapshot, PcmFormat, PlaybackLifecycle, PlaybackOutputDescription,
    RepeatMode,
};
use crate::resolver::TrackResolver;
use crate::sink::AudioSink;

pub struct PlaybackEngine {
    receiver: mpsc::Receiver<EngineCommand>,
    resolver: Arc<dyn TrackResolver>,
    format: PcmFormat,
    state: PlaybackLifecycle,
    generation_id: u64,
    current_track: Option<Track>,
    queue: Vec<Track>,
    queue_index: usize,
    play_order: Vec<usize>,
    play_order_pos: usize,
    base_position_ms: u64,
    playing_started_at: Option<Instant>,
    volume: u8,
    shuffle: bool,
    repeat: RepeatMode,
    output_desc: Option<PlaybackOutputDescription>,
    sinks: Vec<Box<dyn AudioSink>>,
    decoder: Option<FfmpegPcmDecoder>,
    track_bytes_decoded: u64,
    track_pcm_timeline_bytes: u64,
    network_bytes_sent_total: u64,
    output_health: String,
    last_error: Option<String>,
}

impl PlaybackEngine {
    pub fn new(
        receiver: mpsc::Receiver<EngineCommand>,
        resolver: Arc<dyn TrackResolver>,
        format: PcmFormat,
    ) -> Self {
        Self {
            receiver,
            resolver,
            format,
            state: PlaybackLifecycle::Idle,
            generation_id: 0,
            current_track: None,
            queue: Vec::new(),
            queue_index: 0,
            play_order: Vec::new(),
            play_order_pos: 0,
            base_position_ms: 0,
            playing_started_at: None,
            volume: 80,
            shuffle: false,
            repeat: RepeatMode::Off,
            output_desc: None,
            sinks: Vec::new(),
            decoder: None,
            track_bytes_decoded: 0,
            track_pcm_timeline_bytes: 0,
            network_bytes_sent_total: 0,
            output_health: "none".to_string(),
            last_error: None,
        }
    }

    pub fn resolver(&self) -> &Arc<dyn TrackResolver> {
        &self.resolver
    }

    fn recompute_play_order(&mut self, current_idx: usize) {
        if self.queue.is_empty() {
            self.play_order = Vec::new();
            self.play_order_pos = 0;
            return;
        }

        if self.shuffle {
            use rand::seq::SliceRandom;
            let mut order: Vec<usize> = (0..self.queue.len()).collect();
            let mut rng = rand::thread_rng();
            order.shuffle(&mut rng);
            if let Some(pos) = order.iter().position(|&x| x == current_idx) {
                order.swap(0, pos);
            }
            self.play_order = order;
            self.play_order_pos = 0;
        } else {
            self.play_order = (0..self.queue.len()).collect();
            self.play_order_pos = current_idx.min(self.queue.len().saturating_sub(1));
        }
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let mut pos = self.base_position_ms;
        if let Some(started) = self.playing_started_at {
            pos += started.elapsed().as_millis() as u64;
        }
        let duration_ms = self.current_track.as_ref().and_then(|t| t.duration_ms);

        let sinks = self.sinks.iter().map(|s| s.snapshot()).collect();

        EngineSnapshot {
            lifecycle: self.state,
            generation_id: self.generation_id,
            track_id: self.current_track.as_ref().map(|t| t.id),
            current_track: self.current_track.clone(),
            position_ms: pos,
            duration_ms,
            volume: self.volume,
            shuffle: self.shuffle,
            repeat: self.repeat,
            output: self.output_desc.clone(),
            sinks,
            track_bytes_decoded: self.track_bytes_decoded,
            track_pcm_timeline_bytes: self.track_pcm_timeline_bytes,
            network_bytes_sent_total: self.network_bytes_sent_total,
            bytes_decoded: self.track_bytes_decoded,
            bytes_delivered: self.network_bytes_sent_total,
            output_health: self.output_health.clone(),
            last_error: self.last_error.clone(),
            updated_at: Utc::now(),
        }
    }

    pub async fn run(mut self) {
        info!("playback engine task started");
        let chunk_size = self.format.bytes_for_duration_ms(10).max(1920);
        let mut pcm_buf = vec![0u8; chunk_size];

        loop {
            if self.state.is_playing()
                || self.state == PlaybackLifecycle::Preparing
                || self.state == PlaybackLifecycle::Buffering
            {
                tokio::select! {
                    cmd_opt = self.receiver.recv() => {
                        match cmd_opt {
                            Some(cmd) => {
                                if !self.handle_command(cmd).await {
                                    break;
                                }
                            }
                            None => {
                                info!("playback engine command channel closed, shutting down");
                                break;
                            }
                        }
                    }
                    _ = tokio::task::yield_now() => {
                        self.process_audio_chunk(&mut pcm_buf).await;
                    }
                }
            } else {
                match self.receiver.recv().await {
                    Some(cmd) => {
                        if !self.handle_command(cmd).await {
                            break;
                        }
                    }
                    None => {
                        info!("playback engine command channel closed, shutting down");
                        break;
                    }
                }
            }
        }

        self.cleanup().await;
        info!("playback engine task terminated");
    }

    async fn process_audio_chunk(&mut self, pcm_buf: &mut [u8]) {
        let decoder = match self.decoder.as_mut() {
            Some(d) => d,
            None => {
                self.state = PlaybackLifecycle::Stopped;
                return;
            }
        };

        match decoder.read_pcm(pcm_buf).await {
            Ok(0) => {
                info!("decoder reached EOF on current track");
                self.handle_eof().await;
            }
            Ok(n) => {
                self.track_bytes_decoded += n as u64;
                let chunk = &pcm_buf[..n];

                // Concurrent fan-out delivery to all active sinks with bounded per-sink timeout
                let mut write_futs = Vec::with_capacity(self.sinks.len());
                for sink in self.sinks.iter_mut() {
                    write_futs.push(tokio::time::timeout(
                        Duration::from_millis(250),
                        sink.write_pcm(chunk),
                    ));
                }

                let results = futures_util::future::join_all(write_futs).await;

                let mut delivered_bytes = 0usize;
                let mut failed_count = 0usize;

                for (idx, res) in results.into_iter().enumerate() {
                    match res {
                        Ok(Ok(written)) => {
                            delivered_bytes += written;
                        }
                        Ok(Err(e)) => {
                            warn!("sink {} write error: {}", self.sinks[idx].id(), e);
                            failed_count += 1;
                        }
                        Err(_timeout) => {
                            warn!("sink {} write timed out after 250ms", self.sinks[idx].id());
                            failed_count += 1;
                        }
                    }
                }

                if !self.sinks.is_empty() && failed_count == self.sinks.len() {
                    error!("all output sinks failed during playback");
                    self.state = PlaybackLifecycle::Failed;
                    self.output_health = "failed".to_string();
                    self.last_error = Some("all output sinks failed".to_string());
                    if let Some(mut d) = self.decoder.take() {
                        let _ = d.stop().await;
                    }
                    self.playing_started_at = None;
                    return;
                }

                if failed_count > 0 {
                    self.output_health = "partial".to_string();
                }

                self.network_bytes_sent_total += delivered_bytes as u64;

                if delivered_bytes > 0 {
                    // P1-02 Functional Truth: Single unique timeline metric progresses once per decoded chunk
                    self.track_pcm_timeline_bytes += n as u64;

                    if self.state == PlaybackLifecycle::Preparing
                        || self.state == PlaybackLifecycle::Buffering
                    {
                        self.state = PlaybackLifecycle::AudioFlowing;
                        if self.playing_started_at.is_none() {
                            self.playing_started_at = Some(Instant::now());
                        }
                    }

                    // Transition to Playing once unique timeline >= 100ms PCM has been delivered
                    if (self.state == PlaybackLifecycle::AudioFlowing
                        || self.state == PlaybackLifecycle::Preparing)
                        && self.track_pcm_timeline_bytes
                            >= self.format.bytes_for_duration_ms(100) as u64
                    {
                        self.state = PlaybackLifecycle::Playing;
                    }
                }
            }
            Err(e) => {
                error!("decoder error during playback: {}", e);
                self.state = PlaybackLifecycle::Failed;
                self.last_error = Some(e.to_string());
                if let Some(mut d) = self.decoder.take() {
                    let _ = d.stop().await;
                }
                self.playing_started_at = None;
            }
        }
    }

    async fn handle_eof(&mut self) {
        if let Some(mut d) = self.decoder.take() {
            let _ = d.stop().await;
        }
        self.playing_started_at = None;
        self.base_position_ms = 0;

        match self.repeat {
            RepeatMode::One => {
                if let Some(track) = self.current_track.clone() {
                    info!("repeating track {}", track.id);
                    let _ = self.start_playback_internal(track, 0).await;
                    return;
                }
            }
            RepeatMode::All => {
                if !self.queue.is_empty() {
                    if self.play_order_pos + 1 < self.play_order.len() {
                        self.play_order_pos += 1;
                    } else {
                        if self.shuffle {
                            self.recompute_play_order(self.queue_index);
                        }
                        self.play_order_pos = 0;
                    }
                    self.queue_index = self.play_order[self.play_order_pos];
                    let next_track = self.queue[self.queue_index].clone();
                    info!("queue advance (repeat all): track {}", next_track.id);
                    let _ = self.start_playback_internal(next_track, 0).await;
                    return;
                }
            }
            RepeatMode::Off => {
                if !self.queue.is_empty() && self.play_order_pos + 1 < self.play_order.len() {
                    self.play_order_pos += 1;
                    self.queue_index = self.play_order[self.play_order_pos];
                    let next_track = self.queue[self.queue_index].clone();
                    info!("queue advance: next track {}", next_track.id);
                    let _ = self.start_playback_internal(next_track, 0).await;
                    return;
                }
            }
        }

        self.state = PlaybackLifecycle::Ended;
        info!("playback reached end of queue");
    }

    async fn start_playback_internal(
        &mut self,
        track: Track,
        position_ms: u64,
    ) -> Result<(), PlaybackError> {
        if self.sinks.is_empty() {
            self.state = PlaybackLifecycle::Failed;
            self.output_health = "none".to_string();
            self.last_error = Some("no output sinks available".to_string());
            return Err(PlaybackError::NoOutputSelected);
        }

        // Prepare sinks with partial failure tolerance
        let mut prepared = Vec::new();
        let total = self.sinks.len();

        for mut sink in self.sinks.drain(..) {
            match sink.prepare(self.format).await {
                Ok(()) => prepared.push(sink),
                Err(e) => {
                    warn!("sink {} failed to prepare: {}", sink.id(), e);
                }
            }
        }

        if prepared.is_empty() {
            self.state = PlaybackLifecycle::Failed;
            self.output_health = "failed".to_string();
            self.last_error = Some("all output sinks failed to prepare".to_string());
            return Err(PlaybackError::OutputUnavailable(
                "none of the selected sinks are available".to_string(),
            ));
        }

        self.output_health = if prepared.len() == total {
            "healthy".to_string()
        } else {
            "partial".to_string()
        };

        self.sinks = prepared;

        let mut decoder = FfmpegPcmDecoder::new(track.file_path.clone(), self.format);
        decoder.start(position_ms).await?;

        // Reset telemetry for new track generation
        self.generation_id += 1;
        self.track_bytes_decoded = 0;
        self.track_pcm_timeline_bytes = 0;
        self.decoder = Some(decoder);
        self.current_track = Some(track);
        self.base_position_ms = position_ms;
        self.playing_started_at = None;
        self.state = PlaybackLifecycle::Preparing;
        self.last_error = None;

        Ok(())
    }

    async fn handle_command(&mut self, cmd: EngineCommand) -> bool {
        match cmd {
            EngineCommand::Play {
                track,
                sinks,
                output_desc,
                position_ms,
                respond_to,
            } => {
                if sinks.is_empty() {
                    let _ = respond_to.send(Err(PlaybackError::NoOutputSelected));
                    return true;
                }

                self.cleanup_playback().await;

                self.sinks = sinks;
                self.output_desc = Some(output_desc);

                let res = self.start_playback_internal(*track, position_ms).await;
                let _ = respond_to.send(res);
                true
            }
            EngineCommand::LoadTrack {
                track,
                position_ms,
                respond_to,
            } => {
                self.cleanup_playback().await;
                self.generation_id += 1;
                self.track_bytes_decoded = 0;
                self.track_pcm_timeline_bytes = 0;
                self.current_track = Some(*track);
                self.base_position_ms = position_ms;
                self.playing_started_at = None;
                self.state = PlaybackLifecycle::Paused;
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::JumpToIndex { index, respond_to } => {
                if index >= self.queue.len() {
                    let _ = respond_to.send(Err(PlaybackError::QueueIndexInvalid(index)));
                    return true;
                }

                self.queue_index = index;
                self.play_order_pos = self
                    .play_order
                    .iter()
                    .position(|&x| x == index)
                    .unwrap_or(0);
                let target_track = self.queue[index].clone();

                if self.state.is_playing() || self.state == PlaybackLifecycle::Preparing {
                    let res = self.start_playback_internal(target_track, 0).await;
                    let _ = respond_to.send(res);
                } else {
                    self.generation_id += 1;
                    self.track_bytes_decoded = 0;
                    self.track_pcm_timeline_bytes = 0;
                    self.current_track = Some(target_track);
                    self.base_position_ms = 0;
                    self.playing_started_at = None;
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Pause { respond_to } => {
                if let Some(started) = self.playing_started_at.take() {
                    self.base_position_ms += started.elapsed().as_millis() as u64;
                }
                if let Some(mut d) = self.decoder.take() {
                    let _ = d.stop().await;
                }
                let mut pause_errors = 0;
                for sink in self.sinks.iter_mut() {
                    if let Err(e) = sink.pause().await {
                        warn!("sink {} pause error: {}", sink.id(), e);
                        pause_errors += 1;
                    }
                }
                if !self.sinks.is_empty() && pause_errors == self.sinks.len() {
                    self.output_health = "failed".to_string();
                } else if pause_errors > 0 {
                    self.output_health = "partial".to_string();
                }
                self.state = PlaybackLifecycle::Paused;
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::Resume { respond_to } => {
                if self.state == PlaybackLifecycle::Paused {
                    if let Some(track) = self.current_track.clone() {
                        if self.sinks.is_empty() {
                            let _ = respond_to.send(Ok(()));
                        } else {
                            let mut resume_errors = 0;
                            for sink in self.sinks.iter_mut() {
                                if let Err(e) = sink.resume().await {
                                    warn!("sink {} resume error: {}", sink.id(), e);
                                    resume_errors += 1;
                                }
                            }
                            if !self.sinks.is_empty() && resume_errors == self.sinks.len() {
                                self.output_health = "failed".to_string();
                            }
                            let res = self
                                .start_playback_internal(track, self.base_position_ms)
                                .await;
                            let _ = respond_to.send(res);
                        }
                    } else {
                        let _ = respond_to.send(Ok(()));
                    }
                } else if self.state == PlaybackLifecycle::Idle
                    || self.state == PlaybackLifecycle::Stopped
                {
                    if let Some(track) = self.current_track.clone() {
                        if !self.sinks.is_empty() {
                            let res = self
                                .start_playback_internal(track, self.base_position_ms)
                                .await;
                            let _ = respond_to.send(res);
                        } else {
                            let _ = respond_to.send(Ok(()));
                        }
                    } else {
                        let _ = respond_to.send(Ok(()));
                    }
                } else {
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Seek {
                position_ms,
                respond_to,
            } => {
                self.base_position_ms = position_ms;
                if self.state.is_playing() {
                    if let Some(track) = self.current_track.clone() {
                        if let Some(mut d) = self.decoder.take() {
                            let _ = d.stop().await;
                        }
                        let mut decoder =
                            FfmpegPcmDecoder::new(track.file_path.clone(), self.format);
                        match decoder.start(position_ms).await {
                            Ok(()) => {
                                self.decoder = Some(decoder);
                                self.playing_started_at = Some(Instant::now());
                                let _ = respond_to.send(Ok(()));
                            }
                            Err(e) => {
                                self.state = PlaybackLifecycle::Failed;
                                self.last_error = Some(e.to_string());
                                let _ = respond_to.send(Err(e));
                            }
                        }
                    } else {
                        let _ = respond_to.send(Ok(()));
                    }
                } else {
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Next { respond_to } => {
                if !self.queue.is_empty() && self.play_order_pos + 1 < self.play_order.len() {
                    self.play_order_pos += 1;
                    self.queue_index = self.play_order[self.play_order_pos];
                    let next_track = self.queue[self.queue_index].clone();
                    if self.sinks.is_empty() {
                        self.generation_id += 1;
                        self.track_bytes_decoded = 0;
                        self.track_pcm_timeline_bytes = 0;
                        self.current_track = Some(next_track);
                        self.base_position_ms = 0;
                        self.playing_started_at = None;
                        let _ = respond_to.send(Ok(()));
                    } else {
                        let res = self.start_playback_internal(next_track, 0).await;
                        let _ = respond_to.send(res);
                    }
                } else if self.repeat == RepeatMode::All && !self.queue.is_empty() {
                    if self.shuffle {
                        self.recompute_play_order(self.queue_index);
                    }
                    self.play_order_pos = 0;
                    self.queue_index = self.play_order[0];
                    let next_track = self.queue[self.queue_index].clone();
                    if self.sinks.is_empty() {
                        self.generation_id += 1;
                        self.track_bytes_decoded = 0;
                        self.track_pcm_timeline_bytes = 0;
                        self.current_track = Some(next_track);
                        self.base_position_ms = 0;
                        self.playing_started_at = None;
                        let _ = respond_to.send(Ok(()));
                    } else {
                        let res = self.start_playback_internal(next_track, 0).await;
                        let _ = respond_to.send(res);
                    }
                } else {
                    let _ = self.cleanup_playback().await;
                    self.state = PlaybackLifecycle::Ended;
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Previous { respond_to } => {
                if self.base_position_ms > 3000
                    || self
                        .playing_started_at
                        .map(|s| s.elapsed().as_millis() > 3000)
                        .unwrap_or(false)
                {
                    if let Some(track) = self.current_track.clone() {
                        if self.sinks.is_empty() {
                            self.base_position_ms = 0;
                            self.playing_started_at = None;
                            let _ = respond_to.send(Ok(()));
                        } else {
                            let res = self.start_playback_internal(track, 0).await;
                            let _ = respond_to.send(res);
                        }
                    } else {
                        let _ = respond_to.send(Ok(()));
                    }
                } else if self.play_order_pos > 0 && !self.queue.is_empty() {
                    self.play_order_pos -= 1;
                    self.queue_index = self.play_order[self.play_order_pos];
                    let prev_track = self.queue[self.queue_index].clone();
                    if self.sinks.is_empty() {
                        self.generation_id += 1;
                        self.track_bytes_decoded = 0;
                        self.track_pcm_timeline_bytes = 0;
                        self.current_track = Some(prev_track);
                        self.base_position_ms = 0;
                        self.playing_started_at = None;
                        let _ = respond_to.send(Ok(()));
                    } else {
                        let res = self.start_playback_internal(prev_track, 0).await;
                        let _ = respond_to.send(res);
                    }
                } else if let Some(track) = self.current_track.clone() {
                    if self.sinks.is_empty() {
                        self.base_position_ms = 0;
                        self.playing_started_at = None;
                        let _ = respond_to.send(Ok(()));
                    } else {
                        let res = self.start_playback_internal(track, 0).await;
                        let _ = respond_to.send(res);
                    }
                } else {
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Stop { respond_to } => {
                self.cleanup_playback().await;
                self.base_position_ms = 0;
                self.state = PlaybackLifecycle::Stopped;
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::SetVolume { volume, respond_to } => {
                self.volume = volume.min(100);
                for sink in self.sinks.iter_mut() {
                    let _ = sink.set_volume(self.volume).await;
                }
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::SetShuffle {
                shuffle,
                respond_to,
            } => {
                self.shuffle = shuffle;
                self.recompute_play_order(self.queue_index);
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::SetRepeat { repeat, respond_to } => {
                self.repeat = repeat;
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::SetQueue {
                tracks,
                current_index,
                current_track_id,
                respond_to,
            } => {
                let resolved_index = if let Some(target_id) = current_track_id {
                    tracks
                        .iter()
                        .position(|t| t.id == target_id)
                        .unwrap_or(current_index)
                } else {
                    current_index
                };

                self.queue = tracks;
                self.queue_index = resolved_index.min(self.queue.len().saturating_sub(1));
                self.recompute_play_order(self.queue_index);

                if !self.state.is_playing() && self.current_track.is_none() {
                    self.current_track = self.queue.get(self.queue_index).cloned();
                }

                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::GetSnapshot { respond_to } => {
                let _ = respond_to.send(self.snapshot());
                true
            }
            EngineCommand::Shutdown { respond_to } => {
                let _ = respond_to.send(());
                false
            }
        }
    }

    async fn cleanup_playback(&mut self) {
        if let Some(mut d) = self.decoder.take() {
            let _ = d.stop().await;
        }
        for sink in self.sinks.iter_mut() {
            let _ = sink.stop().await;
        }
        self.playing_started_at = None;
    }

    async fn cleanup(&mut self) {
        self.cleanup_playback().await;
        self.sinks.clear();
        self.state = PlaybackLifecycle::Stopped;
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackEngineHandle {
    sender: mpsc::Sender<EngineCommand>,
}

impl PlaybackEngineHandle {
    pub fn new(sender: mpsc::Sender<EngineCommand>) -> Self {
        Self { sender }
    }

    pub async fn play(
        &self,
        track: Track,
        sinks: Vec<Box<dyn AudioSink>>,
        output_desc: PlaybackOutputDescription,
        position_ms: u64,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Play {
                track: Box::new(track),
                sinks,
                output_desc,
                position_ms,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn load_track(&self, track: Track, position_ms: u64) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::LoadTrack {
                track: Box::new(track),
                position_ms,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn jump_to_index(&self, index: usize) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::JumpToIndex {
                index,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn pause(&self) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Pause { respond_to: tx })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn resume(&self) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Resume { respond_to: tx })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn seek(&self, position_ms: u64) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Seek {
                position_ms,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn next(&self) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Next { respond_to: tx })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn previous(&self) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Previous { respond_to: tx })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn stop(&self) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Stop { respond_to: tx })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn set_volume(&self, volume: u8) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::SetVolume {
                volume,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn set_shuffle(&self, shuffle: bool) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::SetShuffle {
                shuffle,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn set_repeat(&self, repeat: RepeatMode) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::SetRepeat {
                repeat,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn set_queue(
        &self,
        tracks: Vec<Track>,
        current_index: usize,
        current_track_id: Option<Uuid>,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::SetQueue {
                tracks,
                current_index,
                current_track_id,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn snapshot(&self) -> Result<EngineSnapshot, PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::GetSnapshot { respond_to: tx })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)
    }

    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self
            .sender
            .send(EngineCommand::Shutdown { respond_to: tx })
            .await
            .is_ok()
        {
            let _ = rx.await;
        }
    }
}

pub fn spawn_playback_engine(
    resolver: Arc<dyn TrackResolver>,
    format: PcmFormat,
) -> (PlaybackEngineHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(64);
    let engine = PlaybackEngine::new(rx, resolver, format);
    let handle = PlaybackEngineHandle::new(tx);
    let join_handle = tokio::spawn(engine.run());
    (handle, join_handle)
}
