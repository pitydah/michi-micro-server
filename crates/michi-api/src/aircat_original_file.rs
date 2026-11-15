//! Source-of-truth original-file reader used by the dedicated AirCat producer.
//! Raw bytes only: never delegate to a transcoding /stream endpoint.
//! Call AFTER Home member authentication, per-library ACL and rate limits.

use crate::aircat_grants::{FileItem, Refusal};
use crate::aircat_producer_security::OriginalAssetRevision;
use michi_core::Track;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

/// Hashes the original file as bounded 64KiB chunks, without loading it fully
/// into RAM, modifying the library or launching a background worker. This is
/// on-demand, and is not allowed on an audio realtime thread.
/// Must compare the file snapshot once more before sending each response.
pub async fn inspect_original_asset(
    music_paths: &[PathBuf],
    track: &Track,
) -> Result<(FileItem, OriginalAssetRevision), Refusal> {
    let (_canonical_path, mut original) =
        michi_streaming::open_track_file_async(music_paths, track)
            .await
            .map_err(|_| Refusal::NotOriginalBytes)?;
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
    };
    Ok((item, revision))
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
