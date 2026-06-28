use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use futures_util::Stream;
use michi_core::{AudioFormat, Track};
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TranscodeFormat {
    Mp3,
    Ogg,
    Hls,
}

impl TranscodeFormat {
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Ogg => "audio/ogg",
            Self::Hls => "application/vnd.apple.mpegurl",
        }
    }

    pub fn ffmpeg_format(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
            Self::Hls => "hls",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
            Self::Hls => "m3u8",
        }
    }
}

impl FromStr for TranscodeFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mp3" => Ok(Self::Mp3),
            "ogg" => Ok(Self::Ogg),
            "hls" => Ok(Self::Hls),
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
    use futures_util::StreamExt;
    use tokio::process::Command;
    use tokio_util::io::ReaderStream;

    let fmt = format.ffmpeg_format().to_string();

    let mut child = Command::new("ffmpeg")
        .arg("-i")
        .arg(file_path)
        .arg("-f")
        .arg(&fmt)
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(StreamError::Io)?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| StreamError::Io(io::Error::other("failed to capture ffmpeg stdout")))?;

    Ok(ReaderStream::new(stdout).map(|r| r.map(|b| b.to_vec())))
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
        assert_eq!(mime_type_for_ext("aiff"), "audio/aiff");
        assert_eq!(mime_type_for_ext("aif"), "audio/aiff");
        assert_eq!(mime_type_for_ext("dsf"), "audio/dsf");
        assert_eq!(mime_type_for_ext("dff"), "audio/dff");
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
}
