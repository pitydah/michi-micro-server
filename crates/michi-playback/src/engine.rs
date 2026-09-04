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
    RepeatMode, SinkSnapshot, SinkState,
};
use crate::resolver::TrackResolver;
use crate::sink::AudioSink;

/// Playing means sustained PCM delivery, not a single successful packet burst.
pub const PLAYING_EVIDENCE_THRESHOLD_MS: u64 = 100;
pub const MAX_FAILED_SINK_SNAPSHOTS: usize = 32;

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
    failed_sinks: Vec<SinkSnapshot>,
    decoder: Option<FfmpegPcmDecoder>,
    track_bytes_decoded: u64,
    track_pcm_timeline_bytes: u64,
    network_bytes_sent_total: u64,
    output_health: String,
    last_error: Option<String>,
    event_tx: tokio::sync::broadcast::Sender<crate::model::TrackedEngineEvent>,
    last_checkpoint: Instant,
    current_command_origin: crate::model::CommandOrigin,
}

impl PlaybackEngine {
    pub fn new(
        receiver: mpsc::Receiver<EngineCommand>,
        resolver: Arc<dyn TrackResolver>,
        format: PcmFormat,
    ) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(128);
        Self::new_with_events(receiver, resolver, format, event_tx)
    }

    pub fn new_with_events(
        receiver: mpsc::Receiver<EngineCommand>,
        resolver: Arc<dyn TrackResolver>,
        format: PcmFormat,
        event_tx: tokio::sync::broadcast::Sender<crate::model::TrackedEngineEvent>,
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
            failed_sinks: Vec::new(),
            decoder: None,
            track_bytes_decoded: 0,
            track_pcm_timeline_bytes: 0,
            network_bytes_sent_total: 0,
            output_health: "none".to_string(),
            last_error: None,
            event_tx,
            last_checkpoint: Instant::now(),
            current_command_origin: crate::model::CommandOrigin::Local,
        }
    }

    pub fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::model::TrackedEngineEvent> {
        self.event_tx.subscribe()
    }

    fn emit_event(&self, event: crate::model::EngineEvent) {
        let _ = self.event_tx.send(crate::model::TrackedEngineEvent {
            event,
            origin: self.current_command_origin.clone(),
        });
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

    fn recompute_play_order_new_cycle(&mut self, just_finished_idx: usize) {
        if self.queue.is_empty() {
            self.play_order = Vec::new();
            self.play_order_pos = 0;
            return;
        }

        if self.shuffle {
            use rand::seq::SliceRandom;
            use rand::Rng;
            let mut order: Vec<usize> = (0..self.queue.len()).collect();
            let mut rng = rand::thread_rng();
            order.shuffle(&mut rng);

            // Mandatory Invariant: when queue.len() > 1, first track of new cycle != just_finished_idx
            if self.queue.len() > 1 && order[0] == just_finished_idx {
                let swap_idx = rng.gen_range(1..self.queue.len());
                order.swap(0, swap_idx);
            }
            self.play_order = order;
            self.play_order_pos = 0;
        } else {
            self.play_order = (0..self.queue.len()).collect();
            self.play_order_pos = 0;
        }
    }

    pub fn calculate_current_position_ms(&self) -> u64 {
        let mut pos = self.base_position_ms;
        if let Some(started) = self.playing_started_at {
            pos += started.elapsed().as_millis() as u64;
        }
        pos
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let pos = self.calculate_current_position_ms();
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
            failed_sinks: self.failed_sinks.clone(),
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
                let mut surviving_sinks = Vec::new();
                let total_sinks = self.sinks.len();
                let old_sinks = std::mem::take(&mut self.sinks);

                for (mut sink, res) in old_sinks.into_iter().zip(results.into_iter()) {
                    match res {
                        Ok(Ok(written)) => {
                            delivered_bytes += written;
                            surviving_sinks.push(sink);
                        }
                        Ok(Err(e)) => {
                            warn!(
                                "sink {} write error ({}), removing from active fanout",
                                sink.id(),
                                e
                            );
                            let mut snap = sink.snapshot();
                            snap.state = SinkState::Failed;
                            snap.last_error = Some(e.to_string());
                            self.record_failed_sink(snap);
                            tokio::spawn(async move {
                                let _ =
                                    tokio::time::timeout(Duration::from_millis(500), sink.stop())
                                        .await;
                            });
                        }
                        Err(_timeout) => {
                            warn!(
                                "sink {} write timed out after 250ms, removing from active fanout",
                                sink.id()
                            );
                            let mut snap = sink.snapshot();
                            snap.state = SinkState::Failed;
                            snap.last_error = Some("write timed out after 250ms".to_string());
                            self.record_failed_sink(snap);
                            tokio::spawn(async move {
                                let _ =
                                    tokio::time::timeout(Duration::from_millis(500), sink.stop())
                                        .await;
                            });
                        }
                    }
                }

                if surviving_sinks.is_empty() {
                    error!("all output sinks failed during playback");
                    self.sinks = Vec::new();
                    self.state = PlaybackLifecycle::Failed;
                    self.output_health = "failed".to_string();
                    self.last_error = Some("all output sinks failed".to_string());
                    if let Some(mut d) = self.decoder.take() {
                        let _ = d.stop().await;
                    }
                    self.playing_started_at = None;
                    self.emit_event(crate::model::EngineEvent::Failed {
                        error: "all output sinks failed".to_string(),
                    });
                    self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                        lifecycle: PlaybackLifecycle::Failed,
                        track_id: self.current_track.as_ref().map(|t| t.id),
                    });
                    return;
                }

                if surviving_sinks.len() < total_sinks {
                    self.output_health = "partial".to_string();
                    self.emit_event(crate::model::EngineEvent::OutputChanged {
                        output_health: self.output_health.clone(),
                    });
                }
                self.sinks = surviving_sinks;

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

                    if self.state == PlaybackLifecycle::AudioFlowing
                        && self.track_pcm_timeline_bytes
                            >= self
                                .format
                                .bytes_for_duration_ms(PLAYING_EVIDENCE_THRESHOLD_MS)
                                as u64
                    {
                        self.state = PlaybackLifecycle::Playing;
                        self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                            lifecycle: PlaybackLifecycle::Playing,
                            track_id: self.current_track.as_ref().map(|t| t.id),
                        });
                    }

                    if (self.state == PlaybackLifecycle::Playing
                        || self.state == PlaybackLifecycle::AudioFlowing)
                        && self.last_checkpoint.elapsed() >= Duration::from_millis(2500)
                    {
                        self.last_checkpoint = Instant::now();
                        let pos = self.calculate_current_position_ms();
                        self.emit_event(crate::model::EngineEvent::PositionCheckpoint {
                            track_id: self.current_track.as_ref().map(|t| t.id),
                            position_ms: pos,
                        });
                    }
                }
            }
            Err(e) => {
                error!("decoder error during playback: {}", e);
                self.state = PlaybackLifecycle::Failed;
                self.output_health = "failed".to_string();
                self.last_error = Some(e.to_string());
                if let Some(mut d) = self.decoder.take() {
                    let _ = d.stop().await;
                }
                self.playing_started_at = None;
                self.emit_event(crate::model::EngineEvent::Failed {
                    error: e.to_string(),
                });
                self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                    lifecycle: PlaybackLifecycle::Failed,
                    track_id: self.current_track.as_ref().map(|t| t.id),
                });
            }
        }
    }

    fn record_failed_sink(&mut self, snap: SinkSnapshot) {
        if self.failed_sinks.len() >= MAX_FAILED_SINK_SNAPSHOTS {
            self.failed_sinks.remove(0);
        }
        self.failed_sinks.push(snap);
    }

    async fn fail_playback(&mut self, e: PlaybackError) {
        warn!("playback failed: {}", e);
        if let Some(mut d) = self.decoder.take() {
            let _ = d.stop().await;
        }
        for sink in self.sinks.iter_mut() {
            let _ = sink.stop().await;
        }
        self.state = PlaybackLifecycle::Failed;
        self.last_error = Some(e.to_string());
        self.playing_started_at = None;
        self.emit_event(crate::model::EngineEvent::Failed {
            error: e.to_string(),
        });
        self.emit_event(crate::model::EngineEvent::LifecycleChanged {
            lifecycle: PlaybackLifecycle::Failed,
            track_id: self.current_track.as_ref().map(|t| t.id),
        });
    }

    async fn handle_eof(&mut self) {
        info!("playback reached EOF, transitioning next track");
        if let Some(mut d) = self.decoder.take() {
            let _ = d.stop().await;
        }
        self.emit_event(crate::model::EngineEvent::Ended {
            track_id: self.current_track.as_ref().map(|t| t.id),
        });

        match self.repeat {
            RepeatMode::One => {
                if let Some(track) = self.current_track.clone() {
                    info!("repeat one: replaying track {}", track.id);
                    if let Err(e) = self.start_playback_internal(track, 0).await {
                        self.fail_playback(e).await;
                    }
                    return;
                }
            }
            RepeatMode::All => {
                if !self.queue.is_empty() {
                    if self.play_order_pos + 1 >= self.play_order.len() {
                        if self.shuffle {
                            self.recompute_play_order_new_cycle(self.queue_index);
                        } else {
                            self.play_order_pos = 0;
                        }
                    } else {
                        self.play_order_pos += 1;
                    }
                    self.queue_index = self.play_order[self.play_order_pos];
                    let next_track = self.queue[self.queue_index].clone();
                    info!("repeat all: advancing to track {}", next_track.id);
                    if let Err(e) = self.start_playback_internal(next_track, 0).await {
                        self.fail_playback(e).await;
                    }
                    return;
                }
            }
            RepeatMode::Off => {
                if !self.queue.is_empty() && self.play_order_pos + 1 < self.play_order.len() {
                    self.play_order_pos += 1;
                    self.queue_index = self.play_order[self.play_order_pos];
                    let next_track = self.queue[self.queue_index].clone();
                    info!("queue advance: next track {}", next_track.id);
                    if let Err(e) = self.start_playback_internal(next_track, 0).await {
                        self.fail_playback(e).await;
                    }
                    return;
                }
            }
        }

        self.state = PlaybackLifecycle::Ended;
        self.emit_event(crate::model::EngineEvent::LifecycleChanged {
            lifecycle: PlaybackLifecycle::Ended,
            track_id: None,
        });
        info!("playback reached end of queue");
    }

    async fn start_playback_internal(
        &mut self,
        track: Track,
        position_ms: u64,
    ) -> Result<(), PlaybackError> {
        self.failed_sinks.clear();
        if self.sinks.is_empty() {
            self.state = PlaybackLifecycle::Failed;
            self.output_health = "none".to_string();
            self.last_error = Some("no output sinks available".to_string());
            self.emit_event(crate::model::EngineEvent::Failed {
                error: "no output sinks available".to_string(),
            });
            self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                lifecycle: PlaybackLifecycle::Failed,
                track_id: Some(track.id),
            });
            return Err(PlaybackError::NoOutputSelected);
        }

        // Prepare sinks concurrently with bounded timeout (P1-05)
        let total = self.sinks.len();
        let format = self.format;
        let prepare_futures: Vec<_> = self
            .sinks
            .drain(..)
            .map(|mut sink| async move {
                let res =
                    tokio::time::timeout(std::time::Duration::from_secs(5), sink.prepare(format))
                        .await;
                match res {
                    Ok(Ok(())) => Ok(sink),
                    Ok(Err(e)) => {
                        warn!("sink {} failed to prepare: {}", sink.id(), e);
                        tokio::spawn(async move {
                            let _ =
                                tokio::time::timeout(Duration::from_millis(500), sink.stop()).await;
                        });
                        Err(())
                    }
                    Err(_) => {
                        warn!("sink {} timed out during prepare", sink.id());
                        tokio::spawn(async move {
                            let _ =
                                tokio::time::timeout(Duration::from_millis(500), sink.stop()).await;
                        });
                        Err(())
                    }
                }
            })
            .collect();

        let results = futures_util::future::join_all(prepare_futures).await;
        let mut prepared = Vec::new();
        for sink in results.into_iter().flatten() {
            prepared.push(sink);
        }

        if prepared.is_empty() {
            self.state = PlaybackLifecycle::Failed;
            self.output_health = "failed".to_string();
            self.last_error = Some("all output sinks failed to prepare".to_string());
            self.emit_event(crate::model::EngineEvent::Failed {
                error: "all output sinks failed to prepare".to_string(),
            });
            self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                lifecycle: PlaybackLifecycle::Failed,
                track_id: Some(track.id),
            });
            return Err(PlaybackError::OutputUnavailable(
                "none of the selected sinks are available".to_string(),
            ));
        }

        self.output_health = if prepared.len() == total {
            "healthy".to_string()
        } else {
            "partial".to_string()
        };

        let mut decoder = FfmpegPcmDecoder::new(track.file_path.clone(), self.format);
        // Rollback prepared sinks if decoder fails (P1-06)
        if let Err(e) = decoder.start(position_ms).await {
            warn!("decoder start failed ({}), stopping prepared sinks", e);
            for sink in prepared.iter_mut() {
                if let Err(stop_err) = sink.stop().await {
                    warn!("rollback stop error for sink {}: {}", sink.id(), stop_err);
                }
            }
            self.sinks = Vec::new();
            self.state = PlaybackLifecycle::Idle;
            self.output_health = "none".to_string();
            self.last_error = Some(e.to_string());
            self.emit_event(crate::model::EngineEvent::Failed {
                error: e.to_string(),
            });
            self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                lifecycle: PlaybackLifecycle::Idle,
                track_id: Some(track.id),
            });
            return Err(e);
        }

        self.sinks = prepared;

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
        self.last_checkpoint = Instant::now();

        self.emit_event(crate::model::EngineEvent::LifecycleChanged {
            lifecycle: self.state,
            track_id: self.current_track.as_ref().map(|t| t.id),
        });
        self.emit_event(crate::model::EngineEvent::TrackChanged {
            track_id: self.current_track.as_ref().map(|t| t.id),
            index: self.queue_index,
        });
        self.emit_event(crate::model::EngineEvent::OutputChanged {
            output_health: self.output_health.clone(),
        });

        Ok(())
    }

    async fn handle_command(&mut self, cmd: EngineCommand) -> bool {
        self.current_command_origin = cmd.origin();
        let res = self.handle_command_inner(cmd).await;
        self.current_command_origin = crate::model::CommandOrigin::Local;
        res
    }

    async fn handle_command_inner(&mut self, cmd: EngineCommand) -> bool {
        match cmd {
            EngineCommand::Play {
                track,
                sinks,
                output_desc,
                position_ms,
                origin: _,
                respond_to,
            } => {
                if sinks.is_empty() {
                    let _ = respond_to.send(Err(PlaybackError::NoOutputSelected));
                    return true;
                }

                let _ = self.cleanup_playback().await;

                self.sinks = sinks;
                self.output_desc = Some(output_desc);

                let res = self.start_playback_internal(*track, position_ms).await;
                let _ = respond_to.send(res);
                true
            }
            EngineCommand::LoadTrack {
                track,
                position_ms,
                origin: _,
                respond_to,
            } => {
                let _ = self.cleanup_playback().await;
                self.current_track = Some(*track);
                self.base_position_ms = position_ms;
                self.state = PlaybackLifecycle::Paused;
                self.emit_event(crate::model::EngineEvent::TrackChanged {
                    track_id: self.current_track.as_ref().map(|t| t.id),
                    index: self.queue_index,
                });
                self.emit_event(crate::model::EngineEvent::Paused {
                    track_id: self.current_track.as_ref().map(|t| t.id),
                    position_ms,
                });
                self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                    lifecycle: self.state,
                    track_id: self.current_track.as_ref().map(|t| t.id),
                });
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::JumpToIndex {
                index,
                origin: _,
                respond_to,
            } => {
                if self.queue.is_empty() || index >= self.queue.len() {
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

                if (self.state.is_playing() || self.state == PlaybackLifecycle::Preparing)
                    && !self.sinks.is_empty()
                {
                    let res = self.start_playback_internal(target_track, 0).await;
                    let _ = respond_to.send(res);
                } else {
                    self.generation_id += 1;
                    self.track_bytes_decoded = 0;
                    self.track_pcm_timeline_bytes = 0;
                    self.current_track = Some(target_track);
                    self.base_position_ms = 0;
                    self.emit_event(crate::model::EngineEvent::TrackChanged {
                        track_id: self.current_track.as_ref().map(|t| t.id),
                        index: self.queue_index,
                    });
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Pause {
                origin: _,
                respond_to,
            } => {
                let pos = self.calculate_current_position_ms();
                if let Some(ref mut d) = self.decoder {
                    let _ = d.stop().await;
                }
                self.decoder = None;
                self.playing_started_at = None;
                self.base_position_ms = pos;
                let mut pause_errors = 0;
                for sink in self.sinks.iter_mut() {
                    if let Err(e) = sink.pause().await {
                        warn!("sink {} pause error: {}", sink.id(), e);
                        pause_errors += 1;
                    }
                }
                if pause_errors == self.sinks.len() && !self.sinks.is_empty() {
                    self.output_health = "failed".to_string();
                }
                self.state = PlaybackLifecycle::Paused;
                self.emit_event(crate::model::EngineEvent::Paused {
                    track_id: self.current_track.as_ref().map(|t| t.id),
                    position_ms: pos,
                });
                self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                    lifecycle: self.state,
                    track_id: self.current_track.as_ref().map(|t| t.id),
                });
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::Resume {
                origin: _,
                respond_to,
            } => {
                if self.sinks.is_empty() {
                    let _ = respond_to.send(Err(PlaybackError::NoOutputSelected));
                    return true;
                }

                if self.state == PlaybackLifecycle::Paused {
                    if let Some(track) = self.current_track.clone() {
                        let mut resume_errors = 0;
                        for sink in self.sinks.iter_mut() {
                            if let Err(e) = sink.resume().await {
                                warn!("sink {} resume error: {}", sink.id(), e);
                                resume_errors += 1;
                            }
                        }
                        if resume_errors == self.sinks.len() {
                            self.output_health = "failed".to_string();
                        }
                        let res = self
                            .start_playback_internal(track, self.base_position_ms)
                            .await;
                        let _ = respond_to.send(res);
                    } else {
                        let _ = respond_to.send(Err(PlaybackError::PlaybackFailed(
                            "no track to resume".to_string(),
                        )));
                    }
                } else if self.state == PlaybackLifecycle::Idle
                    || self.state == PlaybackLifecycle::Stopped
                {
                    if let Some(track) = self.current_track.clone() {
                        let res = self
                            .start_playback_internal(track, self.base_position_ms)
                            .await;
                        let _ = respond_to.send(res);
                    } else {
                        let _ = respond_to.send(Err(PlaybackError::PlaybackFailed(
                            "no track loaded".to_string(),
                        )));
                    }
                } else {
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Seek {
                position_ms,
                origin: _,
                respond_to,
            } => {
                if self.state.is_playing() {
                    if let Some(track) = self.current_track.clone() {
                        if let Some(mut d) = self.decoder.take() {
                            let _ = d.stop().await;
                        }
                        self.generation_id += 1;
                        self.track_bytes_decoded = 0;
                        self.track_pcm_timeline_bytes = 0;
                        self.failed_sinks.clear();
                        self.playing_started_at = None;
                        self.state = PlaybackLifecycle::Preparing;

                        let mut decoder =
                            FfmpegPcmDecoder::new(track.file_path.clone(), self.format);
                        match decoder.start(position_ms).await {
                            Ok(()) => {
                                self.decoder = Some(decoder);
                                self.base_position_ms = position_ms;
                                self.emit_event(crate::model::EngineEvent::Seeked {
                                    track_id: Some(track.id),
                                    position_ms,
                                });
                                self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                                    lifecycle: self.state,
                                    track_id: Some(track.id),
                                });
                                let _ = respond_to.send(Ok(()));
                            }
                            Err(e) => {
                                self.state = PlaybackLifecycle::Failed;
                                self.decoder = None;
                                self.last_error = Some(e.to_string());
                                self.emit_event(crate::model::EngineEvent::Failed {
                                    error: e.to_string(),
                                });
                                self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                                    lifecycle: self.state,
                                    track_id: Some(track.id),
                                });
                                let _ = respond_to.send(Err(e));
                            }
                        }
                    } else {
                        let _ = respond_to.send(Ok(()));
                    }
                } else {
                    self.base_position_ms = position_ms;
                    self.emit_event(crate::model::EngineEvent::Seeked {
                        track_id: self.current_track.as_ref().map(|t| t.id),
                        position_ms,
                    });
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Next {
                origin: _,
                respond_to,
            } => {
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
                        self.emit_event(crate::model::EngineEvent::TrackChanged {
                            track_id: self.current_track.as_ref().map(|t| t.id),
                            index: self.queue_index,
                        });
                        let _ = respond_to.send(Ok(()));
                    } else {
                        let res = self.start_playback_internal(next_track, 0).await;
                        let _ = respond_to.send(res);
                    }
                } else if self.repeat == RepeatMode::All && !self.queue.is_empty() {
                    if self.shuffle {
                        self.recompute_play_order_new_cycle(self.queue_index);
                    } else {
                        self.play_order_pos = 0;
                    }
                    self.queue_index = self.play_order[0];
                    let next_track = self.queue[self.queue_index].clone();
                    if self.sinks.is_empty() {
                        self.generation_id += 1;
                        self.track_bytes_decoded = 0;
                        self.track_pcm_timeline_bytes = 0;
                        self.current_track = Some(next_track);
                        self.base_position_ms = 0;
                        self.playing_started_at = None;
                        self.emit_event(crate::model::EngineEvent::TrackChanged {
                            track_id: self.current_track.as_ref().map(|t| t.id),
                            index: self.queue_index,
                        });
                        let _ = respond_to.send(Ok(()));
                    } else {
                        let res = self.start_playback_internal(next_track, 0).await;
                        let _ = respond_to.send(res);
                    }
                } else {
                    let _ = self.cleanup_playback().await;
                    self.state = PlaybackLifecycle::Ended;
                    self.emit_event(crate::model::EngineEvent::Ended {
                        track_id: self.current_track.as_ref().map(|t| t.id),
                    });
                    self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                        lifecycle: PlaybackLifecycle::Ended,
                        track_id: None,
                    });
                    let _ = respond_to.send(Ok(()));
                }
                true
            }
            EngineCommand::Previous {
                origin: _,
                respond_to,
            } => {
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
                        self.emit_event(crate::model::EngineEvent::TrackChanged {
                            track_id: self.current_track.as_ref().map(|t| t.id),
                            index: self.queue_index,
                        });
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
            EngineCommand::Stop {
                origin: _,
                respond_to,
            } => {
                let res = self.cleanup_playback().await;
                self.base_position_ms = 0;
                self.state = PlaybackLifecycle::Stopped;
                self.emit_event(crate::model::EngineEvent::Stopped);
                self.emit_event(crate::model::EngineEvent::LifecycleChanged {
                    lifecycle: self.state,
                    track_id: None,
                });
                let _ = respond_to.send(res);
                true
            }
            EngineCommand::SetVolume {
                volume,
                origin: _,
                respond_to,
            } => {
                self.volume = volume.min(100);
                self.emit_event(crate::model::EngineEvent::VolumeChanged {
                    volume: self.volume,
                });
                if self.sinks.is_empty() {
                    let _ = respond_to.send(Ok(()));
                } else {
                    let mut errors = 0;
                    let total = self.sinks.len();
                    for sink in self.sinks.iter_mut() {
                        if let Err(e) = sink.set_volume(self.volume).await {
                            warn!("sink {} set_volume error: {}", sink.id(), e);
                            errors += 1;
                        }
                    }
                    if errors == total {
                        self.output_health = "failed".to_string();
                        let _ = respond_to.send(Err(PlaybackError::AllOutputsFailed));
                    } else {
                        if errors > 0 {
                            self.output_health = "partial".to_string();
                        }
                        let _ = respond_to.send(Ok(()));
                    }
                }
                true
            }
            EngineCommand::SetShuffle {
                shuffle,
                origin: _,
                respond_to,
            } => {
                self.shuffle = shuffle;
                self.recompute_play_order(self.queue_index);
                self.emit_event(crate::model::EngineEvent::ShuffleChanged {
                    shuffle: self.shuffle,
                });
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::SetRepeat {
                repeat,
                origin: _,
                respond_to,
            } => {
                self.repeat = repeat;
                self.emit_event(crate::model::EngineEvent::RepeatChanged {
                    repeat: self.repeat,
                });
                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::SetQueue {
                tracks,
                current_index,
                current_track_id,
                origin: _,
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

                self.emit_event(crate::model::EngineEvent::QueueChanged {
                    len: self.queue.len(),
                });
                self.emit_event(crate::model::EngineEvent::TrackChanged {
                    track_id: self.current_track.as_ref().map(|t| t.id),
                    index: self.queue_index,
                });

                let _ = respond_to.send(Ok(()));
                true
            }
            EngineCommand::GetSnapshot { respond_to } => {
                let _ = respond_to.send(self.snapshot());
                true
            }
            EngineCommand::Shutdown { respond_to } => {
                self.cleanup().await;
                let _ = respond_to.send(());
                false
            }
        }
    }

    async fn cleanup_playback(&mut self) -> Result<(), PlaybackError> {
        if let Some(mut d) = self.decoder.take() {
            let _ = d.stop().await;
        }
        let total = self.sinks.len();
        let mut stop_errors = 0;
        let mut first_error = None;
        for sink in self.sinks.iter_mut() {
            if let Err(e) = sink.stop().await {
                warn!("sink {} stop error: {}", sink.id(), e);
                stop_errors += 1;
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
        self.playing_started_at = None;
        if total > 0 && stop_errors == total {
            self.output_health = "failed".to_string();
            Err(first_error.unwrap_or_else(|| {
                PlaybackError::PlaybackFailed("all sinks failed to stop".to_string())
            }))
        } else if stop_errors > 0 {
            self.output_health = "partial".to_string();
            Ok(())
        } else {
            Ok(())
        }
    }

    async fn cleanup(&mut self) {
        let _ = self.cleanup_playback().await;
        self.sinks.clear();
        self.state = PlaybackLifecycle::Stopped;
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackEngineHandle {
    sender: mpsc::Sender<EngineCommand>,
    event_tx: tokio::sync::broadcast::Sender<crate::model::TrackedEngineEvent>,
}

impl PlaybackEngineHandle {
    pub fn new(sender: mpsc::Sender<EngineCommand>) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(128);
        Self { sender, event_tx }
    }

    pub fn new_with_events(
        sender: mpsc::Sender<EngineCommand>,
        event_tx: tokio::sync::broadcast::Sender<crate::model::TrackedEngineEvent>,
    ) -> Self {
        Self { sender, event_tx }
    }

    pub fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::model::TrackedEngineEvent> {
        self.event_tx.subscribe()
    }

    pub async fn play(
        &self,
        track: Track,
        sinks: Vec<Box<dyn AudioSink>>,
        output_desc: PlaybackOutputDescription,
        position_ms: u64,
    ) -> Result<(), PlaybackError> {
        self.play_with_origin(
            track,
            sinks,
            output_desc,
            position_ms,
            crate::model::CommandOrigin::Local,
        )
        .await
    }

    pub async fn play_with_origin(
        &self,
        track: Track,
        sinks: Vec<Box<dyn AudioSink>>,
        output_desc: PlaybackOutputDescription,
        position_ms: u64,
        origin: crate::model::CommandOrigin,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Play {
                track: Box::new(track),
                sinks,
                output_desc,
                position_ms,
                origin,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn load_track(&self, track: Track, position_ms: u64) -> Result<(), PlaybackError> {
        self.load_track_with_origin(track, position_ms, crate::model::CommandOrigin::Local)
            .await
    }

    pub async fn load_track_with_origin(
        &self,
        track: Track,
        position_ms: u64,
        origin: crate::model::CommandOrigin,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::LoadTrack {
                track: Box::new(track),
                position_ms,
                origin,
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
                origin: crate::model::CommandOrigin::Local,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn pause(&self) -> Result<(), PlaybackError> {
        self.pause_with_origin(crate::model::CommandOrigin::Local)
            .await
    }

    pub async fn pause_with_origin(
        &self,
        origin: crate::model::CommandOrigin,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Pause {
                origin,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn resume(&self) -> Result<(), PlaybackError> {
        self.resume_with_origin(crate::model::CommandOrigin::Local)
            .await
    }

    pub async fn resume_with_origin(
        &self,
        origin: crate::model::CommandOrigin,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Resume {
                origin,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn seek(&self, position_ms: u64) -> Result<(), PlaybackError> {
        self.seek_with_origin(position_ms, crate::model::CommandOrigin::Local)
            .await
    }

    pub async fn seek_with_origin(
        &self,
        position_ms: u64,
        origin: crate::model::CommandOrigin,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Seek {
                position_ms,
                origin,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn next(&self) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Next {
                origin: crate::model::CommandOrigin::Local,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn previous(&self) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Previous {
                origin: crate::model::CommandOrigin::Local,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn stop(&self) -> Result<(), PlaybackError> {
        self.stop_with_origin(crate::model::CommandOrigin::Local)
            .await
    }

    pub async fn stop_with_origin(
        &self,
        origin: crate::model::CommandOrigin,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::Stop {
                origin,
                respond_to: tx,
            })
            .await
            .map_err(|_| PlaybackError::ChannelClosed)?;
        rx.await.map_err(|_| PlaybackError::ChannelClosed)?
    }

    pub async fn set_volume(&self, volume: u8) -> Result<(), PlaybackError> {
        self.set_volume_with_origin(volume, crate::model::CommandOrigin::Local)
            .await
    }

    pub async fn set_volume_with_origin(
        &self,
        volume: u8,
        origin: crate::model::CommandOrigin,
    ) -> Result<(), PlaybackError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::SetVolume {
                volume,
                origin,
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
                origin: crate::model::CommandOrigin::Local,
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
                origin: crate::model::CommandOrigin::Local,
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
                origin: crate::model::CommandOrigin::Local,
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
    let (event_tx, _) = tokio::sync::broadcast::channel(128);
    let engine = PlaybackEngine::new_with_events(rx, resolver, format, event_tx.clone());
    let handle = PlaybackEngineHandle::new_with_events(tx, event_tx);
    let join_handle = tokio::spawn(engine.run());
    (handle, join_handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyResolver;
    #[async_trait::async_trait]
    impl TrackResolver for DummyResolver {
        async fn get_track(&self, track_id: Uuid) -> Result<Track, PlaybackError> {
            Err(PlaybackError::TrackNotFound(track_id))
        }
    }

    fn make_dummy_track(title: &str) -> Track {
        Track {
            id: Uuid::new_v4(),
            title: Some(title.to_string()),
            artist: Some("Michi".to_string()),
            album: Some("Album".to_string()),
            album_artist: Some("Michi".to_string()),
            duration_ms: Some(1000),
            file_path: format!("/tmp/{title}.wav"),
            format: michi_core::AudioFormat::Wav,
            sample_rate: Some(48000),
            bit_depth: Some(16),
            channels: Some(2),
            artwork_id: None,
            genre: None,
            year: Some(2026),
            track_number: Some(1),
            disc_number: Some(1),
            content_hash: None,
            file_size: Some(100),
            file_mtime_ns: None,
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_shuffle_repeat_all_does_not_immediately_repeat_finished_track() {
        let (_tx, rx) = mpsc::channel(16);
        let mut engine = PlaybackEngine::new(rx, Arc::new(DummyResolver), PcmFormat::default());
        engine.queue = vec![
            make_dummy_track("A"),
            make_dummy_track("B"),
            make_dummy_track("C"),
            make_dummy_track("D"),
        ];
        engine.shuffle = true;
        engine.repeat = RepeatMode::All;

        for finished_idx in 0..4 {
            for _ in 0..50 {
                engine.recompute_play_order_new_cycle(finished_idx);
                assert_ne!(
                    engine.play_order[0], finished_idx,
                    "first track of new cycle ({}) must not immediately repeat finished track ({})",
                    engine.play_order[0], finished_idx
                );
            }
        }
    }

    #[test]
    fn test_shuffle_repeat_all_each_cycle_contains_all_tracks_once() {
        let (_tx, rx) = mpsc::channel(16);
        let mut engine = PlaybackEngine::new(rx, Arc::new(DummyResolver), PcmFormat::default());
        let n = 6;
        engine.queue = (0..n).map(|i| make_dummy_track(&format!("T{i}"))).collect();
        engine.shuffle = true;
        engine.repeat = RepeatMode::All;

        for _ in 0..100 {
            engine.recompute_play_order_new_cycle(2);
            assert_eq!(engine.play_order.len(), n);
            let mut sorted = engine.play_order.clone();
            sorted.sort_unstable();
            assert_eq!(
                sorted,
                (0..n).collect::<Vec<_>>(),
                "must contain each index exactly once"
            );
        }
    }

    #[test]
    fn test_shuffle_single_track_repeat_all_may_repeat_same_track() {
        let (_tx, rx) = mpsc::channel(16);
        let mut engine = PlaybackEngine::new(rx, Arc::new(DummyResolver), PcmFormat::default());
        engine.queue = vec![make_dummy_track("OnlyTrack")];
        engine.shuffle = true;
        engine.repeat = RepeatMode::All;

        engine.recompute_play_order_new_cycle(0);
        assert_eq!(engine.play_order, vec![0]);
        assert_eq!(engine.play_order_pos, 0);
    }
}
