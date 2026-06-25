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

    let tagged_file = match read_from_path(path) {
        Ok(f) => f,
        Err(e) => {
            error!("failed to read file {}: {}", path.display(), e);
            return Ok(AudioMetadata {
                format,
                ..Default::default()
            });
        }
    };

    let properties = tagged_file.properties();

    let duration_ms = properties.duration().as_millis() as u64;
    let sample_rate = properties.sample_rate();
    let bit_depth = properties.bit_depth();
    let channels = properties.channels();

    let tag = tagged_file.first_tag();

    let (title, artist, album, album_artist, genre, year, track_number, disc_number, has_artwork) =
        match tag {
            Some(tag) => {
                let title = tag.title().map(|s| s.to_string());
                let artist = tag.artist().map(|s| s.to_string());
                let album = tag.album().map(|s| s.to_string());
                let album_artist = tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string());
                let genre = tag.genre().map(|s| s.to_string());
                let year = tag.year().map(|y| y as i32);
                let track_number = tag.track();
                let disc_number = tag.disk();
                let has_artwork = tag.pictures().len() > 0;

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
                )
            }
            None => {
                warn!("no tags found for {}", path.display());
                (None, None, None, None, None, None, None, None, false)
            }
        };

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
    })
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
