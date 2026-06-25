# Audio Metadata

Michi Micro Server uses the [Lofty](https://github.com/Serial-AT/lofty) crate for reading audio metadata.

## Track ID Strategy

Track IDs are stable **UUID v5** identifiers, not random UUID v4. This ensures that re-scanning the same files always yields the same IDs.

The ID is generated from the **relative path** of the file within the music library root:

```
library_root = /music
file_path    = /music/Pink Floyd/Time.flac
relative     = Pink Floyd/Time.flac
ID           = UUID v5(NAMESPACE_URL, "Pink Floyd/Time.flac")
```

This means:
- If the library root changes (e.g., `/music` → `/mnt/music`), the ID **remains the same** because the relative path is unchanged.
- The `file_path` field in the database still stores the **absolute path** for direct file access by the server.

### Fallback

If the relative path cannot be computed (e.g., the file is outside the library root), the ID falls back to the full normalized path as before.

## Supported Formats

| Format  | Extension | Tag Support |
|---------|-----------|-------------|
| MP3     | .mp3      | ID3v2, ID3v1|
| FLAC    | .flac     | Vorbis Comment|
| Ogg Vorbis | .ogg  | Vorbis Comment|
| Opus    | .opus     | Vorbis Comment|
| AAC     | .aac      | ADTS, ID3v2|
| M4A/AAC | .m4a     | MP4 (iTunes)|
| WAV     | .wav      | RIFF Info|
| AIFF    | .aiff, .aif| AIFF Text|
| DSF     | .dsf      | ID3v2|
| DFF     | .dff      | ID3v2|

## Extracted Metadata

For each audio file, the following fields are extracted:

- **title** — Track title
- **artist** — Track artist
- **album** — Album name
- **album_artist** — Album artist
- **genre** — Music genre
- **year** — Release year
- **track_number** — Track number on the album
- **disc_number** — Disc number
- **duration_ms** — Duration in milliseconds
- **sample_rate** — Sample rate in Hz
- **bit_depth** — Bits per sample
- **channels** — Number of audio channels
- **format** — Audio format enum
- **has_artwork** — Whether embedded artwork is present

## Error Handling

The library provides two functions:
- `read_metadata()` — propagates `LoftyError` as `Err` for callers that need to distinguish error types.
- `read_metadata_safe()` — swallows errors and returns `AudioMetadata::default()` (the scanner uses this variant).

The scanner is resilient to individual file failures:
- If metadata cannot be read for a file, a warning is logged and the scanner continues with the next file.
- The file is still registered in the library with unknown metadata.
- Symlinks are skipped entirely to prevent accidental traversal outside the library.
- If a directory cannot be read, a warning is logged and that subtree is skipped.
- The scanner never panics or stops the full scan due to a single corrupt or unreadable file.
