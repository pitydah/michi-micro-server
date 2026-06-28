# Inspirations

Michi Micro Server is independently developed. The following projects have inspired aspects of its design and feature set. No code is copied unless explicitly stated in the file headers and in `THIRD_PARTY_NOTICES.md`.

| Project | Influence | License | Code Copied |
|---------|-----------|---------|-------------|
| [Navidrome](https://github.com/navidrome/navidrome) | Library scanning resilience, Subsonic compatibility, transcoding behavior | GPL-3.0 | None |
| [OpenSubsonic API](https://opensubsonic.netlify.app/) | Endpoint naming, response contracts, client compatibility | Apache-2.0 | None (implementation from spec) |
| [LocalSend](https://github.com/localsend/protocol) | LAN discovery, device pairing, REST transfer model | Apache-2.0 | None |
| [Snapcast](https://github.com/badaix/snapcast) | Multiroom audio backend integration via FIFO/JSON-RPC | GPL-3.0 | None |
| [Music Assistant](https://github.com/music-assistant/hass-music-assistant) | Provider/player/queue/room architecture pattern | Apache-2.0 | None |

## Design Principles (derived from influences)

- **Local-first**: All features work on LAN without internet.
- **Incremental sync**: Transfers only deltas, not full libraries.
- **Decoupled playback**: Server serves files; clients play them.
- **Open compatibility**: OpenSubsonic/Subsonic API allows any client to connect.
- **Self-contained**: No external database, no cloud dependency.
