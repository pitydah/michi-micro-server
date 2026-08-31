use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use futures_util::Stream;
use michi_core::{AudioFormat, StreamProfile, Track};
use michi_db::DbError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("file not found on disk: {0}")]
    FileNotFound(String),

    #[error("file outside music library: {0}")]
    UnsafePath(String),

    #[error("database error: {0}")]
    Database(#[from] DbError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid range header: {0}")]
    InvalidRange(String),
}

#[derive(Debug, Clone, Copy)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
    pub total: u64,
}

impl ByteRange {
    pub fn content_length(&self) -> u64 {
        self.end - self.start + 1
    }

    pub fn content_range_header(&self) -> String {
        format!("bytes {}-{}/{}", self.start, self.end, self.total)
    }
}

pub fn parse_range(header: &str, file_size: u64) -> Result<ByteRange, StreamError> {
    let header = header.trim();

    let rest = header
        .strip_prefix("bytes=")
        .ok_or_else(|| StreamError::InvalidRange("must start with 'bytes='".into()))?;

    let rest = rest.trim();

    let Some((start_str, end_str)) = rest.split_once('-') else {
        return Err(StreamError::InvalidRange("missing '-' in range".into()));
    };

    let start_str = start_str.trim();
    let end_str = end_str.trim();

    if start_str.is_empty() && end_str.is_empty() {
        return Err(StreamError::InvalidRange("empty range".into()));
    }

    if file_size == 0 {
        return Err(StreamError::InvalidRange(
            "range not satisfiable for empty file".into(),
        ));
    }

    if start_str.is_empty() {
        let suffix: u64 = end_str
            .parse()
            .map_err(|_| StreamError::InvalidRange("invalid suffix range".into()))?;
        if suffix == 0 {
            return Err(StreamError::InvalidRange("suffix range of zero".into()));
        }
        let start = file_size.saturating_sub(suffix);
        let end = file_size - 1;
        if start >= file_size {
            return Err(StreamError::InvalidRange(
                "suffix range out of bounds".into(),
            ));
        }
        return Ok(ByteRange {
            start,
            end,
            total: file_size,
        });
    }

    let start: u64 = start_str
        .parse()
        .map_err(|_| StreamError::InvalidRange("invalid start offset".into()))?;

    if start >= file_size {
        return Err(StreamError::InvalidRange("start beyond file size".into()));
    }

    let end = if end_str.is_empty() {
        file_size - 1
    } else {
        let e: u64 = end_str
            .parse()
            .map_err(|_| StreamError::InvalidRange("invalid end offset".into()))?;
        if e >= file_size {
            file_size - 1
        } else {
            e
        }
    };

    if start > end {
        return Err(StreamError::InvalidRange("start after end".into()));
    }

    Ok(ByteRange {
        start,
        end,
        total: file_size,
    })
}

pub fn mime_type_for_ext(ext: &str) -> &'static str {
    AudioFormat::from_extension(ext).mime_type()
}

pub fn validate_track_path(
    music_paths: &[PathBuf],
    file_path: &Path,
) -> Result<PathBuf, StreamError> {
    let canonical_file = file_path.canonicalize().map_err(|_| {
        StreamError::FileNotFound(format!("file not found: {}", file_path.display()))
    })?;

    for music_path in music_paths {
        let canonical_base = match music_path.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if canonical_file.starts_with(&canonical_base) {
            return Ok(canonical_file.clone());
        }
    }

    Err(StreamError::UnsafePath(format!(
        "file {} is outside all configured music libraries",
        canonical_file.display()
    )))
}

pub async fn open_track_file_async(
    music_paths: &[PathBuf],
    track: &Track,
) -> Result<(PathBuf, tokio::fs::File), StreamError> {
    let file_path = Path::new(&track.file_path);
    let canonical = validate_track_path(music_paths, file_path)?;

    if !canonical.is_file() {
        return Err(StreamError::FileNotFound(format!(
            "file does not exist: {}",
            canonical.display()
        )));
    }

    let file = tokio::fs::File::open(&canonical).await?;
    Ok((canonical, file))
}

pub async fn read_range_from_file_async(
    file: &mut tokio::fs::File,
    range: &ByteRange,
) -> Result<Vec<u8>, StreamError> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncSeekExt;

    const MAX_RANGE_BYTES: u64 = 16 * 1024 * 1024;

    if range.content_length() > MAX_RANGE_BYTES {
        return Err(StreamError::InvalidRange(format!(
            "range too large: {} bytes (max {MAX_RANGE_BYTES})",
            range.content_length()
        )));
    }

    let mut buf = vec![0u8; range.content_length() as usize];

    file.seek(std::io::SeekFrom::Start(range.start)).await?;

    let mut total_read = 0usize;
    while total_read < buf.len() {
        let n = file.read(&mut buf[total_read..]).await?;
        if n == 0 {
            break;
        }
        total_read += n;
    }

    buf.truncate(total_read);
    Ok(buf)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodeCodec {
    Opus,
    Mp3,
    Pcm,
    Ogg,
    Hls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodeContainer {
    Ogg,
    Mp3,
    Wav,
    Hls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodePlan {
    pub codec: TranscodeCodec,
    pub container: TranscodeContainer,
    pub bitrate_bps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
}

impl TranscodePlan {
    pub fn mime_type(&self) -> &'static str {
        match self.container {
            TranscodeContainer::Ogg => "audio/ogg",
            TranscodeContainer::Mp3 => "audio/mpeg",
            TranscodeContainer::Wav => "audio/wav",
            TranscodeContainer::Hls => "application/vnd.apple.mpegurl",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamMode {
    Direct,
    Transcode(TranscodePlan),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamDecision {
    pub mode: StreamMode,
    pub source_format: AudioFormat,
    pub output_mime_type: &'static str,
    pub reason: String,
}

impl StreamDecision {
    pub fn needs_transcode(&self) -> bool {
        matches!(self.mode, StreamMode::Transcode(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TranscodeFormat {
    Mp3,
    Ogg,
    Opus,
    Hls,
    Pcm,
}

impl TranscodeFormat {
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Ogg | Self::Opus => "audio/ogg",
            Self::Hls => "application/vnd.apple.mpegurl",
            Self::Pcm => "audio/wav",
        }
    }

    pub fn ffmpeg_format(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
            Self::Opus => "ogg",
            Self::Hls => "hls",
            Self::Pcm => "wav",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Hls => "m3u8",
            Self::Pcm => "wav",
        }
    }
}

impl FromStr for TranscodeFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mp3" => Ok(Self::Mp3),
            "ogg" => Ok(Self::Ogg),
            "opus" => Ok(Self::Opus),
            "hls" => Ok(Self::Hls),
            "pcm" | "wav" => Ok(Self::Pcm),
            _ => Err(()),
        }
    }
}

pub fn check_ffmpeg() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub async fn transcode_stream(
    file_path: &Path,
    format: &TranscodeFormat,
) -> Result<impl Stream<Item = Result<Vec<u8>, io::Error>>, StreamError> {
    let plan = match format {
        TranscodeFormat::Mp3 => TranscodePlan {
            codec: TranscodeCodec::Mp3,
            container: TranscodeContainer::Mp3,
            bitrate_bps: Some(192000),
            sample_rate_hz: None,
            bit_depth: None,
            channels: Some(2),
        },
        TranscodeFormat::Opus => TranscodePlan {
            codec: TranscodeCodec::Opus,
            container: TranscodeContainer::Ogg,
            bitrate_bps: Some(128000),
            sample_rate_hz: None,
            bit_depth: None,
            channels: Some(2),
        },
        TranscodeFormat::Ogg => TranscodePlan {
            codec: TranscodeCodec::Ogg,
            container: TranscodeContainer::Ogg,
            bitrate_bps: None,
            sample_rate_hz: None,
            bit_depth: None,
            channels: Some(2),
        },
        TranscodeFormat::Pcm => TranscodePlan {
            codec: TranscodeCodec::Pcm,
            container: TranscodeContainer::Wav,
            bitrate_bps: None,
            sample_rate_hz: Some(48000),
            bit_depth: Some(24),
            channels: Some(2),
        },
        TranscodeFormat::Hls => TranscodePlan {
            codec: TranscodeCodec::Hls,
            container: TranscodeContainer::Hls,
            bitrate_bps: None,
            sample_rate_hz: None,
            bit_depth: None,
            channels: Some(2),
        },
    };

    transcode_stream_with_plan(file_path, &plan).await
}

pub async fn transcode_stream_with_plan(
    file_path: &Path,
    plan: &TranscodePlan,
) -> Result<impl Stream<Item = Result<Vec<u8>, io::Error>>, StreamError> {
    use futures_util::StreamExt;
    use tokio::process::Command;
    use tokio_util::io::ReaderStream;

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-i").arg(file_path).arg("-vn");

    match plan.codec {
        TranscodeCodec::Opus => {
            cmd.arg("-c:a").arg("libopus");
            if let Some(bps) = plan.bitrate_bps {
                let kbps = (bps / 1000).max(32);
                cmd.arg("-b:a").arg(format!("{kbps}k"));
            }
            cmd.arg("-f").arg("ogg");
        }
        TranscodeCodec::Mp3 => {
            cmd.arg("-c:a").arg("libmp3lame");
            if let Some(bps) = plan.bitrate_bps {
                let kbps = (bps / 1000).max(32);
                cmd.arg("-b:a").arg(format!("{kbps}k"));
            }
            cmd.arg("-f").arg("mp3");
        }
        TranscodeCodec::Pcm => {
            if let Some(bd) = plan.bit_depth {
                if bd == 24 {
                    cmd.arg("-c:a").arg("pcm_s24le");
                } else {
                    cmd.arg("-c:a").arg("pcm_s16le");
                }
            } else {
                cmd.arg("-c:a").arg("pcm_s16le");
            }
            if let Some(sr) = plan.sample_rate_hz {
                cmd.arg("-ar").arg(sr.to_string());
            }
            cmd.arg("-f").arg("wav");
        }
        TranscodeCodec::Ogg => {
            cmd.arg("-c:a").arg("libvorbis");
            if let Some(bps) = plan.bitrate_bps {
                let kbps = (bps / 1000).max(32);
                cmd.arg("-b:a").arg(format!("{kbps}k"));
            }
            cmd.arg("-f").arg("ogg");
        }
        TranscodeCodec::Hls => {
            cmd.arg("-c").arg("copy").arg("-f").arg("hls");
        }
    }

    if let Some(channels) = plan.channels {
        cmd.arg("-ac").arg(channels.to_string());
    }

    cmd.arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let mut child = cmd.spawn().map_err(StreamError::Io)?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| StreamError::Io(io::Error::other("failed to capture ffmpeg stdout")))?;

    Ok(ReaderStream::new(stdout).map(|r| r.map(|b| b.to_vec())))
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_stream_decision(
    track_format: &AudioFormat,
    track_sample_rate: Option<u32>,
    track_bit_depth: Option<u32>,
    explicit_format: Option<&str>,
    stream_profile: StreamProfile,
    format_policy: michi_core::AudioFormatPolicy,
    resource_profile: michi_core::ResourceProfile,
    max_remote_bitrate: u32,
) -> Result<StreamDecision, String> {
    // 1. Enforce DirectPlay format policy
    if format_policy == michi_core::AudioFormatPolicy::DirectPlay {
        if let Some(req_fmt) = explicit_format {
            let req_lower = req_fmt.to_lowercase();
            let is_same = match track_format {
                AudioFormat::Mp3 => req_lower == "mp3",
                AudioFormat::Flac => req_lower == "flac",
                AudioFormat::Ogg | AudioFormat::Opus => req_lower == "ogg" || req_lower == "opus",
                AudioFormat::Wav => req_lower == "wav",
                AudioFormat::Aac => req_lower == "aac" || req_lower == "m4a",
                _ => false,
            };
            if !is_same {
                return Err(
                    "TRANSCODING_FORBIDDEN_BY_POLICY: DirectPlay format policy forbids transcoding"
                        .into(),
                );
            }
        }
        return Ok(StreamDecision {
            mode: StreamMode::Direct,
            source_format: *track_format,
            output_mime_type: track_format.mime_type(),
            reason: "policy: DirectPlay enforced".into(),
        });
    }

    // 2. Resolve requested format or profile
    let plan_opt: Option<TranscodePlan>;

    if let Some(req_fmt) = explicit_format {
        let req_lower = req_fmt.to_lowercase();
        match req_lower.as_str() {
            "mp3" => {
                if format_policy == michi_core::AudioFormatPolicy::LosslessOnly {
                    return Err("TRANSCODING_FORBIDDEN_BY_POLICY: LosslessOnly policy forbids lossy MP3 transcoding".into());
                }
                let target_bps = 192000.min(max_remote_bitrate);
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Mp3,
                    container: TranscodeContainer::Mp3,
                    bitrate_bps: Some(target_bps),
                    sample_rate_hz: track_sample_rate,
                    bit_depth: None,
                    channels: Some(2),
                });
            }
            "opus" => {
                if format_policy == michi_core::AudioFormatPolicy::LosslessOnly {
                    return Err("TRANSCODING_FORBIDDEN_BY_POLICY: LosslessOnly policy forbids lossy Opus transcoding".into());
                }
                let target_bps = 128000.min(max_remote_bitrate);
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Opus,
                    container: TranscodeContainer::Ogg,
                    bitrate_bps: Some(target_bps),
                    sample_rate_hz: track_sample_rate,
                    bit_depth: None,
                    channels: Some(2),
                });
            }
            "ogg" => {
                if format_policy == michi_core::AudioFormatPolicy::LosslessOnly {
                    return Err("TRANSCODING_FORBIDDEN_BY_POLICY: LosslessOnly policy forbids lossy Ogg transcoding".into());
                }
                let target_bps = 160000.min(max_remote_bitrate);
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Ogg,
                    container: TranscodeContainer::Ogg,
                    bitrate_bps: Some(target_bps),
                    sample_rate_hz: track_sample_rate,
                    bit_depth: None,
                    channels: Some(2),
                });
            }
            "wav" | "pcm" => {
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Pcm,
                    container: TranscodeContainer::Wav,
                    bitrate_bps: None,
                    sample_rate_hz: track_sample_rate,
                    bit_depth: track_bit_depth.map(|d| d as u8).or(Some(16)),
                    channels: Some(2),
                });
            }
            "flac" => {
                if track_format.is_lossless() {
                    return Ok(StreamDecision {
                        mode: StreamMode::Direct,
                        source_format: *track_format,
                        output_mime_type: track_format.mime_type(),
                        reason: "direct lossless play".into(),
                    });
                } else {
                    return Err("UNSUPPORTED_TRANSCODE_PLAN: transcoding lossy source to FLAC is not supported".into());
                }
            }
            _ => {
                return Err(format!(
                    "INVALID_STREAM_PROFILE: requested format '{req_fmt}' is not supported"
                ));
            }
        }
    } else {
        // Resolve from configured StreamProfile
        match stream_profile {
            StreamProfile::Original | StreamProfile::Custom => {
                return Ok(StreamDecision {
                    mode: StreamMode::Direct,
                    source_format: *track_format,
                    output_mime_type: track_format.mime_type(),
                    reason: "profile: original".into(),
                });
            }
            StreamProfile::LosslessCompatible => {
                if track_format.is_lossless() {
                    return Ok(StreamDecision {
                        mode: StreamMode::Direct,
                        source_format: *track_format,
                        output_mime_type: track_format.mime_type(),
                        reason: "profile: lossless compatible direct play".into(),
                    });
                } else {
                    return Ok(StreamDecision {
                        mode: StreamMode::Direct,
                        source_format: *track_format,
                        output_mime_type: track_format.mime_type(),
                        reason: "profile: direct play source".into(),
                    });
                }
            }
            StreamProfile::OpusMobile96 => {
                if format_policy == michi_core::AudioFormatPolicy::LosslessOnly {
                    return Err(
                        "TRANSCODING_FORBIDDEN_BY_POLICY: LosslessOnly policy forbids OpusMobile96"
                            .into(),
                    );
                }
                let bps = 96000.min(max_remote_bitrate);
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Opus,
                    container: TranscodeContainer::Ogg,
                    bitrate_bps: Some(bps),
                    sample_rate_hz: track_sample_rate,
                    bit_depth: None,
                    channels: Some(2),
                });
            }
            StreamProfile::OpusMobile160 => {
                if format_policy == michi_core::AudioFormatPolicy::LosslessOnly {
                    return Err("TRANSCODING_FORBIDDEN_BY_POLICY: LosslessOnly policy forbids OpusMobile160".into());
                }
                let bps = 160000.min(max_remote_bitrate);
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Opus,
                    container: TranscodeContainer::Ogg,
                    bitrate_bps: Some(bps),
                    sample_rate_hz: track_sample_rate,
                    bit_depth: None,
                    channels: Some(2),
                });
            }
            StreamProfile::Mp3Compatibility192 => {
                if format_policy == michi_core::AudioFormatPolicy::LosslessOnly {
                    return Err("TRANSCODING_FORBIDDEN_BY_POLICY: LosslessOnly policy forbids Mp3Compatibility192".into());
                }
                let bps = 192000.min(max_remote_bitrate);
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Mp3,
                    container: TranscodeContainer::Mp3,
                    bitrate_bps: Some(bps),
                    sample_rate_hz: track_sample_rate,
                    bit_depth: None,
                    channels: Some(2),
                });
            }
            StreamProfile::Mp3Compatibility320 => {
                if format_policy == michi_core::AudioFormatPolicy::LosslessOnly {
                    return Err("TRANSCODING_FORBIDDEN_BY_POLICY: LosslessOnly policy forbids Mp3Compatibility320".into());
                }
                let bps = 320000.min(max_remote_bitrate);
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Mp3,
                    container: TranscodeContainer::Mp3,
                    bitrate_bps: Some(bps),
                    sample_rate_hz: track_sample_rate,
                    bit_depth: None,
                    channels: Some(2),
                });
            }
            StreamProfile::Downsample2448 => {
                let target_sr = track_sample_rate.unwrap_or(48000).min(48000);
                let target_bd = track_bit_depth.unwrap_or(24).min(24) as u8;
                plan_opt = Some(TranscodePlan {
                    codec: TranscodeCodec::Pcm,
                    container: TranscodeContainer::Wav,
                    bitrate_bps: None,
                    sample_rate_hz: Some(target_sr),
                    bit_depth: Some(target_bd),
                    channels: Some(2),
                });
            }
        }
    }

    if let Some(plan) = plan_opt {
        // Check ResourceProfile transcode permissions
        if resource_profile.max_transcodes() == 0 {
            return Err(
                "TRANSCODING_DISABLED: current resource profile (Eco) does not allow transcoding"
                    .into(),
            );
        }

        let mime = plan.mime_type();
        Ok(StreamDecision {
            mode: StreamMode::Transcode(plan),
            source_format: *track_format,
            output_mime_type: mime,
            reason: format!("transcode to {stream_profile:?} via profile {stream_profile:?}"),
        })
    } else {
        Ok(StreamDecision {
            mode: StreamMode::Direct,
            source_format: *track_format,
            output_mime_type: track_format.mime_type(),
            reason: "direct play".into(),
        })
    }
}

pub const HLS_SEGMENT_DURATION: u64 = 10;

pub fn hls_output_dir(cache_path: &Path, track_id: &str) -> PathBuf {
    cache_path.join("hls").join(track_id)
}

pub async fn generate_hls_playlist(
    file_path: &Path,
    cache_path: &Path,
    track_id: &str,
) -> Result<(), StreamError> {
    use tokio::process::Command;

    let out_dir = hls_output_dir(cache_path, track_id);
    let _ = tokio::fs::create_dir_all(&out_dir).await;

    let playlist_path = out_dir.join("playlist.m3u8");

    let status = Command::new("ffmpeg")
        .arg("-i")
        .arg(file_path)
        .arg("-c")
        .arg("copy")
        .arg("-f")
        .arg("hls")
        .arg("-hls_time")
        .arg(HLS_SEGMENT_DURATION.to_string())
        .arg("-hls_list_size")
        .arg("0")
        .arg("-hls_segment_filename")
        .arg(out_dir.join("seg_%05d.ts").to_str().unwrap())
        .arg(&playlist_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(StreamError::Io)?
        .wait()
        .await
        .map_err(StreamError::Io)?;

    if !status.success() {
        return Err(StreamError::Io(io::Error::other(
            "ffmpeg hls segmentation failed",
        )));
    }

    Ok(())
}

pub async fn read_hls_playlist(cache_path: &Path, track_id: &str) -> Result<String, StreamError> {
    let path = hls_output_dir(cache_path, track_id).join("playlist.m3u8");
    tokio::fs::read_to_string(&path).await.map_err(|_| {
        StreamError::FileNotFound(format!("HLS playlist not found: {}", path.display()))
    })
}

pub fn hls_segment_path(cache_path: &Path, track_id: &str, segment: &str) -> PathBuf {
    // segment can be "seg_00001.ts" or a full filename
    hls_output_dir(cache_path, track_id).join(segment)
}

pub fn select_stream_profile(
    profile: StreamProfile,
    track_format: &AudioFormat,
    original_sample_rate: Option<u32>,
    original_bit_depth: Option<u32>,
    max_transcodes: usize,
    active_transcodes: usize,
) -> StreamDecision {
    if active_transcodes >= max_transcodes && profile.needs_transcode() {
        return StreamDecision {
            mode: StreamMode::Direct,
            source_format: *track_format,
            output_mime_type: track_format.mime_type(),
            reason: "capacity reached, falling back to direct play".into(),
        };
    }

    let resource_profile = if max_transcodes == 0 {
        michi_core::ResourceProfile::Eco
    } else {
        michi_core::ResourceProfile::Balanced
    };

    resolve_stream_decision(
        track_format,
        original_sample_rate,
        original_bit_depth,
        None,
        profile,
        michi_core::AudioFormatPolicy::StandardOnly,
        resource_profile,
        10_000_000,
    )
    .unwrap_or(StreamDecision {
        mode: StreamMode::Direct,
        source_format: *track_format,
        output_mime_type: track_format.mime_type(),
        reason: "fallback direct play".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_type_for_ext() {
        assert_eq!(mime_type_for_ext("mp3"), "audio/mpeg");
        assert_eq!(mime_type_for_ext("flac"), "audio/flac");
        assert_eq!(mime_type_for_ext("ogg"), "audio/ogg");
        assert_eq!(mime_type_for_ext("opus"), "audio/ogg");
        assert_eq!(mime_type_for_ext("m4a"), "audio/mp4");
        assert_eq!(mime_type_for_ext("aac"), "audio/aac");
        assert_eq!(mime_type_for_ext("wav"), "audio/wav");
        assert_eq!(mime_type_for_ext("aiff"), "application/octet-stream");
        assert_eq!(mime_type_for_ext("aif"), "application/octet-stream");
        assert_eq!(mime_type_for_ext("dsf"), "application/octet-stream");
        assert_eq!(mime_type_for_ext("dff"), "application/octet-stream");
        assert_eq!(mime_type_for_ext("txt"), "application/octet-stream");
    }

    #[test]
    fn test_parse_range_full_prefix() {
        let range = parse_range("bytes=0-1023", 10000).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 1023);
        assert_eq!(range.total, 10000);
        assert_eq!(range.content_length(), 1024);
        assert_eq!(range.content_range_header(), "bytes 0-1023/10000");
    }

    #[test]
    fn test_parse_range_from_offset() {
        let range = parse_range("bytes=100-", 10000).unwrap();
        assert_eq!(range.start, 100);
        assert_eq!(range.end, 9999);
        assert_eq!(range.total, 10000);
    }

    #[test]
    fn test_parse_range_suffix() {
        let range = parse_range("bytes=-500", 10000).unwrap();
        assert_eq!(range.start, 9500);
        assert_eq!(range.end, 9999);
        assert_eq!(range.total, 10000);
        assert_eq!(range.content_length(), 500);
    }

    #[test]
    fn test_parse_range_suffix_larger_than_file() {
        let range = parse_range("bytes=-20000", 10000).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 9999);
        assert_eq!(range.total, 10000);
    }

    #[test]
    fn test_parse_range_end_beyond_file() {
        let range = parse_range("bytes=0-999999", 10000).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 9999);
    }

    #[test]
    fn test_parse_range_start_beyond_file() {
        let result = parse_range("bytes=10000-20000", 10000);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_range_start_after_end() {
        let result = parse_range("bytes=100-50", 10000);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_range_no_bytes_prefix() {
        let result = parse_range("0-1023", 10000);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_range_empty() {
        let result = parse_range("bytes=", 10000);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_range_invalid_start() {
        let result = parse_range("bytes=abc-100", 10000);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_range_zero_suffix() {
        let result = parse_range("bytes=-0", 10000);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_track_path_valid() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("test.flac");
        std::fs::write(&file_path, b"data").unwrap();

        let result = validate_track_path(&[dir.path().to_path_buf()], &file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_track_path_second_path() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let sub = dir2.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("test.flac");
        std::fs::write(&file_path, b"data").unwrap();

        // File is in dir2, should be valid when dir2 is in the list
        let result = validate_track_path(
            &[dir1.path().to_path_buf(), dir2.path().to_path_buf()],
            &file_path,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_track_path_outside() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let outside_file = dir2.path().join("secret.txt");
        std::fs::write(&outside_file, b"secret").unwrap();

        let result = validate_track_path(&[dir1.path().to_path_buf()], &outside_file);
        assert!(result.is_err());
        match result {
            Err(StreamError::UnsafePath(_)) => {}
            _ => panic!("expected UnsafePath error"),
        }
    }

    #[test]
    fn test_validate_track_path_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("nonexistent.flac");
        let result = validate_track_path(&[dir.path().to_path_buf()], &fake);
        assert!(result.is_err());
        match result {
            Err(StreamError::FileNotFound(_)) => {}
            _ => panic!("expected FileNotFound error"),
        }
    }

    #[test]
    fn test_check_ffmpeg_runs_without_panicking() {
        // Just verify calling check_ffmpeg doesn't panic
        let _ = check_ffmpeg();
    }

    #[test]
    fn test_transcode_format_from_str() {
        assert_eq!(
            "mp3".parse::<TranscodeFormat>().unwrap(),
            TranscodeFormat::Mp3
        );
        assert_eq!(
            "MP3".parse::<TranscodeFormat>().unwrap(),
            TranscodeFormat::Mp3
        );
        assert_eq!(
            "ogg".parse::<TranscodeFormat>().unwrap(),
            TranscodeFormat::Ogg
        );
        assert_eq!(
            "OGG".parse::<TranscodeFormat>().unwrap(),
            TranscodeFormat::Ogg
        );
        assert_eq!(
            "hls".parse::<TranscodeFormat>().unwrap(),
            TranscodeFormat::Hls
        );
        assert!("flac".parse::<TranscodeFormat>().is_err());
    }

    #[test]
    fn test_transcode_format_mime_type() {
        assert_eq!(TranscodeFormat::Mp3.mime_type(), "audio/mpeg");
        assert_eq!(TranscodeFormat::Ogg.mime_type(), "audio/ogg");
    }

    #[test]
    fn test_transcode_format_extension() {
        assert_eq!(TranscodeFormat::Mp3.extension(), "mp3");
        assert_eq!(TranscodeFormat::Ogg.extension(), "ogg");
    }

    #[test]
    fn test_parse_range_empty_file() {
        assert!(parse_range("bytes=0-", 0).is_err());
        assert!(parse_range("bytes=0-1023", 0).is_err());
        assert!(parse_range("bytes=-500", 0).is_err());
    }

    #[test]
    fn test_parse_range_huge() {
        let range = parse_range("bytes=0-999999", 1000000).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 999999);
    }

    #[test]
    fn test_parse_range_not_satisfiable() {
        assert!(parse_range("bytes=10000-", 5000).is_err());
        assert!(parse_range("bytes=0-", 0).is_err());
    }

    #[test]
    fn test_parse_range_start_past_end() {
        assert!(parse_range("bytes=100-50", 1000).is_err());
    }

    #[test]
    fn test_validate_track_path_multi() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let sub = dir2.path().join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("track.flac");
        std::fs::write(&file_path, b"data").unwrap();

        let result = validate_track_path(
            &[dir1.path().to_path_buf(), dir2.path().to_path_buf()],
            &file_path,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_track_path_outside_all() {
        let dir1 = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file_path = outside.path().join("secret.flac");
        std::fs::write(&file_path, b"data").unwrap();

        let result = validate_track_path(&[dir1.path().to_path_buf()], &file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_stream_decision_matrix() {
        use michi_core::{AudioFormatPolicy, ResourceProfile};

        // 1. FLAC + Original profile + LosslessOnly -> Direct
        let dec1 = resolve_stream_decision(
            &AudioFormat::Flac,
            Some(44100),
            Some(16),
            None,
            StreamProfile::Original,
            AudioFormatPolicy::LosslessOnly,
            ResourceProfile::Balanced,
            320_000,
        )
        .unwrap();
        assert!(!dec1.needs_transcode());
        assert_eq!(dec1.output_mime_type, "audio/flac");

        // 2. FLAC + OpusMobile160 profile + StandardOnly -> Transcode
        let dec2 = resolve_stream_decision(
            &AudioFormat::Flac,
            Some(44100),
            Some(16),
            None,
            StreamProfile::OpusMobile160,
            AudioFormatPolicy::StandardOnly,
            ResourceProfile::Balanced,
            320_000,
        )
        .unwrap();
        assert!(dec2.needs_transcode());
        assert_eq!(dec2.output_mime_type, "audio/ogg");

        // 3. FLAC + Mp3Compatibility320 profile + StandardOnly policy -> Transcode to MP3 320k
        let dec3 = resolve_stream_decision(
            &AudioFormat::Flac,
            Some(44100),
            Some(16),
            None,
            StreamProfile::Mp3Compatibility320,
            AudioFormatPolicy::StandardOnly,
            ResourceProfile::Balanced,
            320_000,
        )
        .unwrap();
        assert!(dec3.needs_transcode());
        assert_eq!(dec3.output_mime_type, "audio/mpeg");

        // 4. FLAC + Direct policy -> Direct play only
        let dec4 = resolve_stream_decision(
            &AudioFormat::Flac,
            Some(44100),
            Some(16),
            None,
            StreamProfile::Mp3Compatibility320,
            AudioFormatPolicy::DirectPlay,
            ResourceProfile::Balanced,
            320_000,
        )
        .unwrap();
        assert!(!dec4.needs_transcode());
        assert_eq!(dec4.output_mime_type, "audio/flac");

        // 5. Explicit format query "opus" + StandardOnly -> Transcodes to Opus
        let dec5 = resolve_stream_decision(
            &AudioFormat::Flac,
            Some(44100),
            Some(16),
            Some("opus"),
            StreamProfile::Original,
            AudioFormatPolicy::StandardOnly,
            ResourceProfile::Balanced,
            320_000,
        )
        .unwrap();
        assert!(dec5.needs_transcode());
        assert_eq!(dec5.output_mime_type, "audio/ogg");
    }

    #[tokio::test]
    async fn test_real_ffmpeg_transcode_effect_execution() {
        use futures_util::StreamExt;

        if !check_ffmpeg() {
            eprintln!("ffmpeg not available in environment; skipping real effect test");
            return;
        }

        let tmp = tempfile::tempdir().unwrap();
        let wav_path = tmp.path().join("source.wav");

        // Write a valid 44.1kHz 16-bit Stereo PCM WAV file with 1.5s of sine audio
        let sample_rate: u32 = 44100;
        let channels: u16 = 2;
        let bits_per_sample: u16 = 16;
        let num_samples = 66150; // 1.5s
        let data_size = num_samples * (channels as u32) * ((bits_per_sample / 8) as u32);
        let file_size = 36 + data_size;

        let mut wav_bytes = Vec::with_capacity((file_size + 8) as usize);
        wav_bytes.extend_from_slice(b"RIFF");
        wav_bytes.extend_from_slice(&(file_size).to_le_bytes());
        wav_bytes.extend_from_slice(b"WAVEfmt ");
        wav_bytes.extend_from_slice(&(16u32).to_le_bytes()); // subchunk1 size
        wav_bytes.extend_from_slice(&(1u16).to_le_bytes()); // PCM
        wav_bytes.extend_from_slice(&channels.to_le_bytes());
        wav_bytes.extend_from_slice(&sample_rate.to_le_bytes());
        let byte_rate = sample_rate * (channels as u32) * (bits_per_sample as u32 / 8);
        wav_bytes.extend_from_slice(&byte_rate.to_le_bytes());
        let block_align = channels * (bits_per_sample / 8);
        wav_bytes.extend_from_slice(&block_align.to_le_bytes());
        wav_bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav_bytes.extend_from_slice(b"data");
        wav_bytes.extend_from_slice(&data_size.to_le_bytes());
        for i in 0..num_samples {
            let t = (i as f32) / (sample_rate as f32);
            let val = ((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 16000.0) as i16;
            let sample_bytes = val.to_le_bytes();
            wav_bytes.extend_from_slice(&sample_bytes);
            wav_bytes.extend_from_slice(&sample_bytes);
        }

        std::fs::write(&wav_path, &wav_bytes).unwrap();

        // 1. Test Opus 96k Plan
        let opus96_plan = TranscodePlan {
            codec: TranscodeCodec::Opus,
            container: TranscodeContainer::Ogg,
            bitrate_bps: Some(96000),
            sample_rate_hz: Some(48000),
            bit_depth: None,
            channels: Some(2),
        };
        let mut opus96_stream = transcode_stream_with_plan(&wav_path, &opus96_plan)
            .await
            .unwrap();
        let mut opus96_out = Vec::new();
        while let Some(res) = opus96_stream.next().await {
            let chunk = res.unwrap();
            opus96_out.extend_from_slice(&chunk);
        }
        assert!(
            !opus96_out.is_empty(),
            "Opus 96k transcode must output bytes"
        );
        assert_eq!(&opus96_out[0..4], b"OggS", "Opus must start with OggS");
        let opus96_file = tmp.path().join("out_96k.opus");
        std::fs::write(&opus96_file, &opus96_out).unwrap();

        // 2. Test Opus 160k Plan
        let opus160_plan = TranscodePlan {
            codec: TranscodeCodec::Opus,
            container: TranscodeContainer::Ogg,
            bitrate_bps: Some(160000),
            sample_rate_hz: Some(48000),
            bit_depth: None,
            channels: Some(2),
        };
        let mut opus160_stream = transcode_stream_with_plan(&wav_path, &opus160_plan)
            .await
            .unwrap();
        let mut opus160_out = Vec::new();
        while let Some(res) = opus160_stream.next().await {
            let chunk = res.unwrap();
            opus160_out.extend_from_slice(&chunk);
        }
        assert_eq!(
            &opus160_out[0..4],
            b"OggS",
            "Opus 160k must start with OggS"
        );
        let opus160_file = tmp.path().join("out_160k.opus");
        std::fs::write(&opus160_file, &opus160_out).unwrap();

        // 3. Test MP3 192k Plan
        let mp3_192_plan = TranscodePlan {
            codec: TranscodeCodec::Mp3,
            container: TranscodeContainer::Mp3,
            bitrate_bps: Some(192000),
            sample_rate_hz: Some(44100),
            bit_depth: None,
            channels: Some(2),
        };
        let mut mp3_192_stream = transcode_stream_with_plan(&wav_path, &mp3_192_plan)
            .await
            .unwrap();
        let mut mp3_192_out = Vec::new();
        while let Some(res) = mp3_192_stream.next().await {
            let chunk = res.unwrap();
            mp3_192_out.extend_from_slice(&chunk);
        }
        assert!(
            !mp3_192_out.is_empty(),
            "MP3 192k transcode must output bytes"
        );
        let mp3_192_file = tmp.path().join("out_192k.mp3");
        std::fs::write(&mp3_192_file, &mp3_192_out).unwrap();

        // 4. Test MP3 320k Plan
        let mp3_320_plan = TranscodePlan {
            codec: TranscodeCodec::Mp3,
            container: TranscodeContainer::Mp3,
            bitrate_bps: Some(320000),
            sample_rate_hz: Some(44100),
            bit_depth: None,
            channels: Some(2),
        };
        let mut mp3_320_stream = transcode_stream_with_plan(&wav_path, &mp3_320_plan)
            .await
            .unwrap();
        let mut mp3_320_out = Vec::new();
        while let Some(res) = mp3_320_stream.next().await {
            let chunk = res.unwrap();
            mp3_320_out.extend_from_slice(&chunk);
        }
        assert!(
            !mp3_320_out.is_empty(),
            "MP3 320k transcode must output bytes"
        );
        let mp3_320_file = tmp.path().join("out_320k.mp3");
        std::fs::write(&mp3_320_file, &mp3_320_out).unwrap();

        // 5. Test PCM 24/48 Transcode Plan
        let pcm_plan = TranscodePlan {
            codec: TranscodeCodec::Pcm,
            container: TranscodeContainer::Wav,
            bitrate_bps: None,
            sample_rate_hz: Some(48000),
            bit_depth: Some(24),
            channels: Some(2),
        };
        let mut pcm_stream = transcode_stream_with_plan(&wav_path, &pcm_plan)
            .await
            .unwrap();
        let mut pcm_out = Vec::new();
        while let Some(res) = pcm_stream.next().await {
            let chunk = res.unwrap();
            pcm_out.extend_from_slice(&chunk);
        }
        assert_eq!(
            &pcm_out[0..4],
            b"RIFF",
            "PCM WAV output must start with RIFF header"
        );
        assert_eq!(
            &pcm_out[8..12],
            b"WAVE",
            "PCM WAV output must contain WAVE format"
        );
        let pcm_channels = u16::from_le_bytes(pcm_out[22..24].try_into().unwrap());
        let pcm_sample_rate = u32::from_le_bytes(pcm_out[24..28].try_into().unwrap());
        let pcm_bits = u16::from_le_bytes(pcm_out[34..36].try_into().unwrap());
        assert_eq!(pcm_channels, 2, "PCM output must have 2 channels");
        assert_eq!(
            pcm_sample_rate, 48000,
            "PCM output must have 48000Hz sample rate"
        );
        assert_eq!(pcm_bits, 24, "PCM output must have 24 bits per sample");

        // 6. Fail-closed ffprobe parameter and bitrate certification
        let probe = |path: &std::path::Path| -> (String, u16, u32, u64) {
            let output = std::process::Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "stream=codec_name,channels,sample_rate:format=bit_rate",
                    "-of",
                    "default=noprint_wrappers=1:nokey=0",
                    path.to_str().unwrap(),
                ])
                .output()
                .expect("ffprobe execution must succeed");
            assert!(
                output.status.success(),
                "ffprobe must successfully parse {}",
                path.display()
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut codec = String::new();
            let mut channels = 0u16;
            let mut sample_rate = 0u32;
            let mut bit_rate = 0u64;

            for line in stdout.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k.trim() {
                        "codec_name" => codec = v.trim().to_string(),
                        "channels" => channels = v.trim().parse().unwrap_or(0),
                        "sample_rate" => sample_rate = v.trim().parse().unwrap_or(0),
                        "bit_rate" => bit_rate = v.trim().parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            (codec, channels, sample_rate, bit_rate)
        };

        let (c_op96, ch_op96, sr_op96, br_op96) = probe(&opus96_file);
        assert_eq!(c_op96, "opus", "Opus 96k codec mismatch");
        assert_eq!(ch_op96, 2, "Opus 96k channels mismatch");
        assert_eq!(sr_op96, 48000, "Opus 96k sample rate mismatch");
        assert!(
            (70_000..=125_000).contains(&br_op96),
            "Opus 96k bitrate out of bounds: {br_op96} bps"
        );

        let (c_op160, ch_op160, sr_op160, br_op160) = probe(&opus160_file);
        assert_eq!(c_op160, "opus", "Opus 160k codec mismatch");
        assert_eq!(ch_op160, 2, "Opus 160k channels mismatch");
        assert_eq!(sr_op160, 48000, "Opus 160k sample rate mismatch");
        assert!(
            (125_000..=195_000).contains(&br_op160),
            "Opus 160k bitrate out of bounds: {br_op160} bps"
        );

        let (c_mp192, ch_mp192, sr_mp192, br_mp192) = probe(&mp3_192_file);
        assert_eq!(c_mp192, "mp3", "MP3 192k codec mismatch");
        assert_eq!(ch_mp192, 2, "MP3 192k channels mismatch");
        assert_eq!(sr_mp192, 44100, "MP3 192k sample rate mismatch");
        assert!(
            (180_000..=205_000).contains(&br_mp192),
            "MP3 192k bitrate out of bounds: {br_mp192} bps"
        );

        let (c_mp320, ch_mp320, sr_mp320, br_mp320) = probe(&mp3_320_file);
        assert_eq!(c_mp320, "mp3", "MP3 320k codec mismatch");
        assert_eq!(ch_mp320, 2, "MP3 320k channels mismatch");
        assert_eq!(sr_mp320, 44100, "MP3 320k sample rate mismatch");
        assert!(
            (300_000..=340_000).contains(&br_mp320),
            "MP3 320k bitrate out of bounds: {br_mp320} bps"
        );
    }
}
