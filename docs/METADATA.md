# Audio Metadata

Michi Micro Server uses the [Lofty](https://github.com/Serial-AT/lofty) crate for reading audio metadata.

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

The scanner is resilient to individual file failures. If metadata cannot be read for a file, a warning is logged and the scanner continues with the next file. The file is still registered in the library with unknown metadata.
