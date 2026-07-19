use std::path::Path;

use lofty::file::AudioFile;
use lofty::file::TaggedFileExt;
use lofty::read_from_path;
use lofty::tag::Accessor;
use lofty::tag::ItemKey;
use michi_core::{AudioFormat, AudioMetadata};
use thiserror::Error;
use tracing::{error, warn};

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("failed to open file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to read metadata: {0}")]
    Lofty(#[from] lofty::error::LoftyError),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
}

fn format_from_path(path: &Path) -> AudioFormat {
    path.extension()
        .and_then(|e| e.to_str())
        .map(AudioFormat::from_extension)
        .unwrap_or(AudioFormat::Unknown)
}

pub fn read_metadata(path: &Path) -> Result<AudioMetadata, MetadataError> {
    if !path.exists() {
        return Err(MetadataError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", path.display()),
        )));
    }

    let format = format_from_path(path);

    let tagged_file = read_from_path(path)?;

    let properties = tagged_file.properties();

    let duration_ms = properties.duration().as_millis() as u64;
    let sample_rate = properties.sample_rate();
    let bit_depth = properties.bit_depth();
    let channels = properties.channels();

    let tag = tagged_file.first_tag();

    let (
        mut title,
        artist,
        album,
        album_artist,
        genre,
        year,
        track_number,
        disc_number,
        has_artwork,
        replaygain_gain,
        replaygain_peak,
    ) = match tag {
        Some(tag) => {
            let title = tag.title().map(|s| s.to_string());
            let artist = tag.artist().map(|s| s.to_string());
            let album = tag.album().map(|s| s.to_string());
            let album_artist = tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string());
            let genre = tag.genre().map(|s| s.to_string());
            let year = tag.year().map(|y| y as i32);
            let track_number = tag.track();
            let disc_number = tag.disk();
            let has_artwork = !tag.pictures().is_empty();
            // Read ReplayGain from tag items (TXXX frames in ID3, etc.)
            let mut replaygain_gain = None;
            let mut replaygain_peak = None;
            for item in tag.items() {
                let key = format!("{:?}", item.key()).to_uppercase();
                if let Some(text) = item.value().text() {
                    if key.contains("REPLAYGAIN_TRACK_GAIN") {
                        replaygain_gain = text.parse::<f64>().ok();
                    }
                    if key.contains("REPLAYGAIN_TRACK_PEAK") {
                        replaygain_peak = text.parse::<f64>().ok();
                    }
                }
            }

            (
                title,
                artist,
                album,
                album_artist,
                genre,
                year,
                track_number,
                disc_number,
                has_artwork,
                replaygain_gain,
                replaygain_peak,
            )
        }
        None => {
            warn!("no tags found for {}", path.display());
            (
                None, None, None, None, None, None, None, None, false, None, None,
            )
        }
    };

    if title.is_none() {
        title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
    }

    Ok(AudioMetadata {
        title,
        artist,
        album,
        album_artist,
        genre,
        year,
        track_number,
        disc_number,
        duration_ms: (duration_ms > 0).then_some(duration_ms),
        sample_rate,
        bit_depth,
        channels,
        format,
        has_artwork,
        replaygain_track_gain: replaygain_gain,
        replaygain_track_peak: replaygain_peak,
    })
}

pub fn extract_artwork(path: &Path) -> Result<Vec<u8>, MetadataError> {
    let tagged_file = read_from_path(path)?;
    let tag = tagged_file.first_tag().ok_or_else(|| {
        MetadataError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no tags found",
        ))
    })?;
    let picture = tag.pictures().first().ok_or_else(|| {
        MetadataError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no artwork found",
        ))
    })?;
    Ok(picture.data().to_vec())
}

pub fn read_metadata_safe(path: &Path) -> AudioMetadata {
    match read_metadata(path) {
        Ok(meta) => meta,
        Err(e) => {
            error!("error reading metadata from {}: {}", path.display(), e);
            AudioMetadata {
                format: format_from_path(path),
                ..Default::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_format_from_path() {
        assert_eq!(format_from_path(Path::new("song.mp3")), AudioFormat::Mp3);
        assert_eq!(format_from_path(Path::new("song.flac")), AudioFormat::Flac);
        assert_eq!(format_from_path(Path::new("song.ogg")), AudioFormat::Ogg);
        assert_eq!(format_from_path(Path::new("song.wav")), AudioFormat::Wav);
        assert_eq!(format_from_path(Path::new("song.aac")), AudioFormat::Aac);
        assert_eq!(format_from_path(Path::new("song.m4a")), AudioFormat::M4a);
        assert_eq!(format_from_path(Path::new("song.opus")), AudioFormat::Opus);
        assert_eq!(
            format_from_path(Path::new("song.aiff")),
            AudioFormat::Unknown
        );
        assert_eq!(
            format_from_path(Path::new("song.aif")),
            AudioFormat::Unknown
        );
        assert_eq!(
            format_from_path(Path::new("song.dsf")),
            AudioFormat::Unknown
        );
        assert_eq!(
            format_from_path(Path::new("song.dff")),
            AudioFormat::Unknown
        );
        assert_eq!(
            format_from_path(Path::new("song.txt")),
            AudioFormat::Unknown
        );
        assert_eq!(format_from_path(Path::new("song")), AudioFormat::Unknown);
    }

    #[test]
    fn test_read_metadata_nonexistent_file() {
        let result = read_metadata(Path::new("/nonexistent_xyz.flac"));
        assert!(result.is_err());
        assert!(matches!(result, Err(MetadataError::Io(_))));
    }

    #[test]
    fn test_read_metadata_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.flac");
        fs::write(&path, b"not a valid audio file at all").unwrap();
        let result = read_metadata(&path);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetadataError::Lofty(_))));
    }

    #[test]
    fn test_read_metadata_safe_nonexistent() {
        let result = read_metadata_safe(Path::new("/nonexistent_xyz.flac"));
        assert_eq!(result.format, AudioFormat::Flac);
        assert!(result.title.is_none());
    }

    #[test]
    fn test_read_metadata_safe_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.flac");
        fs::write(&path, b"garbage data").unwrap();
        let result = read_metadata_safe(&path);
        assert_eq!(result.format, AudioFormat::Flac);
        assert!(result.title.is_none());
    }
}
