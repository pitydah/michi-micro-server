use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use michi_core::{AudioFormat, Track};
use michi_db::DbError;
use sqlx::SqlitePool;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("invalid track id: {0}")]
    InvalidId(String),

    #[error("track not found: {0}")]
    TrackNotFound(String),

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

pub fn mime_type_for_format(format: &AudioFormat) -> &'static str {
    format.mime_type()
}

pub fn mime_type_for_ext(ext: &str) -> &'static str {
    AudioFormat::from_extension(ext).mime_type()
}

pub fn validate_track_path(music_path: &Path, file_path: &Path) -> Result<PathBuf, StreamError> {
    let canonical_base = music_path
        .canonicalize()
        .map_err(|e| StreamError::UnsafePath(format!("cannot canonicalize music path: {}", e)))?;

    let canonical_file = file_path.canonicalize().map_err(|_| {
        StreamError::FileNotFound(format!("file not found: {}", file_path.display()))
    })?;

    if !canonical_file.starts_with(&canonical_base) {
        return Err(StreamError::UnsafePath(format!(
            "file {} is outside music library {}",
            canonical_file.display(),
            canonical_base.display()
        )));
    }

    Ok(canonical_file)
}

pub fn open_track_file(music_path: &Path, track: &Track) -> Result<(PathBuf, File), StreamError> {
    let file_path = Path::new(&track.file_path);
    let canonical = validate_track_path(music_path, file_path)?;

    if !canonical.is_file() {
        return Err(StreamError::FileNotFound(format!(
            "file does not exist: {}",
            canonical.display()
        )));
    }

    let file = File::open(&canonical)?;
    Ok((canonical, file))
}

pub async fn resolve_track(
    pool: &SqlitePool,
    id_str: &str,
    music_path: &Path,
) -> Result<(Track, PathBuf, File), StreamError> {
    let id = Uuid::from_str(id_str).map_err(|_| StreamError::InvalidId(id_str.to_string()))?;

    let track = michi_db::get_track(pool, &id)
        .await?
        .ok_or_else(|| StreamError::TrackNotFound(id_str.to_string()))?;

    let (path, file) = open_track_file(music_path, &track)?;
    Ok((track, path, file))
}

pub fn read_range_from_file(mut file: &File, range: &ByteRange) -> Result<Vec<u8>, StreamError> {
    let mut buf = vec![0u8; range.content_length() as usize];

    file.seek(SeekFrom::Start(range.start))?;

    let mut total_read = 0usize;
    while total_read < buf.len() {
        let n = file.read(&mut buf[total_read..])?;
        if n == 0 {
            break;
        }
        total_read += n;
    }

    buf.truncate(total_read);
    Ok(buf)
}

pub fn is_valid_range_request(header: &str) -> bool {
    header.trim().starts_with("bytes=")
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
    fn test_is_valid_range_request() {
        assert!(is_valid_range_request("bytes=0-1023"));
        assert!(is_valid_range_request("bytes=100-"));
        assert!(!is_valid_range_request("0-1023"));
        assert!(!is_valid_range_request(""));
    }

    #[test]
    fn test_validate_track_path_valid() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("test.flac");
        std::fs::write(&file_path, b"data").unwrap();

        let result = validate_track_path(dir.path(), &file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_track_path_outside() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let outside_file = dir2.path().join("secret.txt");
        std::fs::write(&outside_file, b"secret").unwrap();

        let result = validate_track_path(dir1.path(), &outside_file);
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
        let result = validate_track_path(dir.path(), &fake);
        assert!(result.is_err());
        match result {
            Err(StreamError::FileNotFound(_)) => {}
            _ => panic!("expected FileNotFound error"),
        }
    }
}
