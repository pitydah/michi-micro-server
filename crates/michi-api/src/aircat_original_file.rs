//! Source-of-truth original-file reader used by the dedicated AirCat producer.
//! Raw bytes only: never delegate to a transcoding /stream endpoint.
//! Call AFTER Home member authentication, per-library ACL and rate limits.

use crate::aircat_grants::{FileItem, Refusal};
use crate::aircat_producer_security::OriginalAssetRevision;
use michi_core::Track;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio::io::AsyncReadExt;

pub fn metadata_dev_ino(meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

pub fn metadata_modified_unix_ns(meta: &std::fs::Metadata) -> Option<i128> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128)
}

pub fn revision_matches_metadata(rev: &OriginalAssetRevision, meta: &std::fs::Metadata) -> bool {
    if !meta.is_file() || meta.len() as i64 != rev.exact_size {
        return false;
    }
    if let (Some(expected), Some(actual)) = (rev.dev_ino, metadata_dev_ino(meta)) {
        if expected != actual {
            return false;
        }
    }
    if let (Some(expected), Some(actual)) = (rev.modified_unix_ns, metadata_modified_unix_ns(meta))
    {
        if expected != actual {
            return false;
        }
    }
    true
}

/// Opens an original track file only if it resides strictly inside `music_paths`,
/// has zero symlink components along the relative path, and is a regular file.
pub async fn open_original_file_no_symlinks(
    music_paths: &[PathBuf],
    track: &Track,
) -> Result<(PathBuf, tokio::fs::File), Refusal> {
    let file_path = Path::new(&track.file_path);
    if track.file_path.is_empty() {
        return Err(Refusal::NotOriginalBytes);
    }

    // Reject if the target path itself is a symlink or non-regular file.
    let target_sym_meta =
        std::fs::symlink_metadata(file_path).map_err(|_| Refusal::NotOriginalBytes)?;
    if target_sym_meta.file_type().is_symlink() || !target_sym_meta.is_file() {
        return Err(Refusal::NotOriginalBytes);
    }

    let canonical_file = file_path
        .canonicalize()
        .map_err(|_| Refusal::NotOriginalBytes)?;

    let mut matched_root: Option<PathBuf> = None;
    for music_path in music_paths {
        let Ok(canonical_root) = music_path.canonicalize() else {
            continue;
        };
        if canonical_file.starts_with(&canonical_root) {
            // Walk every component from canonical_root to the relative target and reject symlinks.
            let rel = file_path
                .strip_prefix(music_path)
                .or_else(|_| canonical_file.strip_prefix(&canonical_root))
                .map_err(|_| Refusal::NotOriginalBytes)?;
            let mut cursor = canonical_root.clone();
            for comp in rel.components() {
                match comp {
                    Component::Normal(part) => {
                        cursor.push(part);
                        let meta = std::fs::symlink_metadata(&cursor)
                            .map_err(|_| Refusal::NotOriginalBytes)?;
                        if meta.file_type().is_symlink() {
                            return Err(Refusal::NotOriginalBytes);
                        }
                    }
                    Component::CurDir => {}
                    _ => return Err(Refusal::NotOriginalBytes),
                }
            }
            matched_root = Some(canonical_file.clone());
            break;
        }
    }

    let safe_path = matched_root.ok_or(Refusal::NotOriginalBytes)?;
    let file = {
        #[cfg(unix)]
        {
            let mut opts = tokio::fs::OpenOptions::new();
            opts.read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            opts.open(&safe_path)
                .await
                .map_err(|_| Refusal::NotOriginalBytes)?
        }
        #[cfg(not(unix))]
        {
            tokio::fs::File::open(&safe_path)
                .await
                .map_err(|_| Refusal::NotOriginalBytes)?
        }
    };
    let opened_meta = file
        .metadata()
        .await
        .map_err(|_| Refusal::NotOriginalBytes)?;
    if !opened_meta.is_file() {
        return Err(Refusal::NotOriginalBytes);
    }
    if let (Some(a), Some(b)) = (
        metadata_dev_ino(&target_sym_meta),
        metadata_dev_ino(&opened_meta),
    ) {
        if a != b {
            return Err(Refusal::NotOriginalBytes);
        }
    }

    Ok((safe_path, file))
}

/// Returns original track file metadata and SHA-256 revision using the bounded
/// in-memory `(size, dev_ino, modified_unix_ns)` cache on `/manifest` calls.
/// Falls back to full 64 KiB streaming SHA-256 hashing on the SAME opened file
/// descriptor whenever the file is unseen or any metadata field changed.
pub async fn inspect_original_asset_cached(
    music_paths: &[PathBuf],
    track: &Track,
    security: &crate::aircat_producer_security::AirCatProducerSecurityState,
) -> Result<(FileItem, OriginalAssetRevision), Refusal> {
    let (_canonical_path, mut original) =
        open_original_file_no_symlinks(music_paths, track).await?;
    let meta = original
        .metadata()
        .await
        .map_err(|_| Refusal::NotOriginalBytes)?;
    let remote_id = track.id.to_string();
    if let Some(cached) = security.cached_original_if_fresh(&remote_id, &meta) {
        let item = FileItem {
            logical_track_id: remote_id.clone(),
            remote_track_id: remote_id,
            title: track.title.clone().unwrap_or_default(),
            artist: track.artist.clone().unwrap_or_default(),
            album: track.album.clone().unwrap_or_default(),
            format: track.format.as_str().to_owned(),
            size: cached.exact_size,
            sha256: cached.exact_sha256.clone(),
        };
        return Ok((item, cached));
    }
    let (item, revision) = inspect_opened_original_file(track, &mut original).await?;
    security.cache_original_revision(revision.clone());
    Ok((item, revision))
}

/// Hashes an already-opened original file descriptor as bounded 64KiB chunks,
/// without reopening the path, loading it into RAM, or touching playback threads.
async fn inspect_opened_original_file(
    track: &Track,
    original: &mut tokio::fs::File,
) -> Result<(FileItem, OriginalAssetRevision), Refusal> {
    let before = original
        .metadata()
        .await
        .map_err(|_| Refusal::NotOriginalBytes)?;
    if !before.is_file() || before.len() == 0 || before.len() > i64::MAX as u64 {
        return Err(Refusal::NotOriginalBytes);
    }
    let modified_before = before.modified().map_err(|_| Refusal::NotOriginalBytes)?;
    let mut sha = Sha256::new();
    let mut total: u64 = 0;
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let n = original
            .read(&mut bytes)
            .await
            .map_err(|_| Refusal::NotOriginalBytes)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(Refusal::NotOriginalBytes)?;
        sha.update(&bytes[..n]);
    }
    let after = original
        .metadata()
        .await
        .map_err(|_| Refusal::NotOriginalBytes)?;
    let modified_after = after.modified().map_err(|_| Refusal::NotOriginalBytes)?;
    if total != before.len() || after.len() != before.len() || modified_after != modified_before {
        return Err(Refusal::NotOriginalBytes);
    }
    let digest = hex::encode(sha.finalize());
    let size = total as i64;
    let remote_id = track.id.to_string();
    let item = FileItem {
        logical_track_id: remote_id.clone(),
        remote_track_id: remote_id.clone(),
        title: track.title.clone().unwrap_or_default(),
        artist: track.artist.clone().unwrap_or_default(),
        album: track.album.clone().unwrap_or_default(),
        format: track.format.as_str().to_owned(),
        size,
        sha256: digest.clone(),
    };
    let revision = OriginalAssetRevision {
        remote_track_id: remote_id,
        exact_size: size,
        exact_sha256: digest.clone(),
        strong_etag: format!("\"sha256-{digest}-{size}\""),
        dev_ino: metadata_dev_ino(&after),
        modified_unix_ns: metadata_modified_unix_ns(&after),
    };
    Ok((item, revision))
}

/// Hashes the original file as bounded 64KiB chunks, without loading it fully
/// into RAM, modifying the library or launching a background worker. This is
/// on-demand, and is not allowed on an audio realtime thread.
/// Must compare the file snapshot once more before sending each response.
pub async fn inspect_original_asset(
    music_paths: &[PathBuf],
    track: &Track,
) -> Result<(FileItem, OriginalAssetRevision), Refusal> {
    let (_canonical_path, mut original) =
        open_original_file_no_symlinks(music_paths, track).await?;
    inspect_opened_original_file(track, &mut original).await
}

/// A revision is a digest receipt, not merely mtime or file length. Before
/// emitting 200/206, a producer MUST still bind the actual opened descriptor
/// to the verified revision, or copy into immutable storage. A changed file
/// rejects with 409 rather than mixing bytes from revisions.
pub fn expected_strong_etag(sha256: &str, bytes: i64) -> Result<String, Refusal> {
    if bytes <= 0
        || sha256.len() != 64
        || !sha256
            .bytes()
            .all(|x| x.is_ascii_hexdigit() && !x.is_ascii_uppercase())
    {
        return Err(Refusal::NotOriginalBytes);
    }
    Ok(format!("\"sha256-{sha256}-{bytes}\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisions_are_strong_quoted_and_content_bound() {
        let sha = "a".repeat(64);
        assert_eq!(
            expected_strong_etag(&sha, 123).unwrap(),
            format!("\"sha256-{sha}-123\"")
        );
        assert!(expected_strong_etag("notsha", 123).is_err());
        assert!(expected_strong_etag(&sha, 0).is_err());
    }
}
