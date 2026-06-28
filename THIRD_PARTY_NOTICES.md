# Third-Party Notices

Michi Micro Server is licensed under GPL-3.0-only.

## OpenSubsonic API

Michi Micro Server implements an independent compatibility layer based on the OpenSubsonic API specification.

- **Source**: [OpenSubsonic API](https://opensubsonic.netlify.app/)
- **License**: Apache-2.0
- **Use**: endpoint naming, response model compatibility, client compatibility behavior
- **Copied code**: none, unless specified per file.
- **Changes**: Respuesta en JSON en lugar de XML por defecto; envelope adaptado a naming de Rust.

## LocalSend Protocol

Michi Sync is inspired by LocalSend's local-first device discovery and REST transfer model.

- **Source**: [LocalSend](https://github.com/localsend/protocol)
- **License**: Apache-2.0
- **Use**: LAN discovery, device identity, transfer lifecycle concepts
- **Copied code**: none.

## Snapcast

Michi Rooms integrates with Snapcast as an external multiroom audio backend.

- **Source**: [Snapcast](https://github.com/badaix/snapcast)
- **License**: GPL-3.0
- **Use**: external service integration via FIFO and JSON-RPC
- **Copied code**: none.

## Navidrome

Michi Micro Server is conceptually inspired by Navidrome's lightweight self-hosted music server model.

- **Source**: [Navidrome](https://github.com/navidrome/navidrome)
- **License**: GPL-3.0
- **Use**: feature inspiration for library scanning, Subsonic compatibility, transcoding and metadata behavior
- **Copied code**: none.

## Music Assistant

Michi Micro Server adopts an independent provider/player/queue/room architecture inspired by Music Assistant.

- **Source**: [Music Assistant](https://github.com/music-assistant/hass-music-assistant)
- **License**: Apache-2.0
- **Use**: architectural inspiration for home-audio server orchestration
- **Copied code**: none.
