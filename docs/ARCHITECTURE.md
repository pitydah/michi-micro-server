# Architecture

Michi Micro Server is organized as a Rust workspace with a modular crate structure.

## Workspace Layout

```
michi-micro-server/
├── apps/
│   └── michi-server/       # Main binary
├── crates/
│   ├── michi-core/         # Shared models and types
│   ├── michi-api/          # HTTP routes (Axum)
│   ├── michi-config/       # Configuration from env vars
│   ├── michi-db/           # SQLite database layer (SQLx)
│   ├── michi-metadata/     # Audio metadata reading (Lofty)
│   ├── michi-scanner/      # Music library scanner
│   ├── michi-streaming/    # Audio streaming (future)
│   ├── michi-homeassistant/# Home Assistant MQTT (future)
│   ├── michi-sync/         # Sync with Michi players (future)
│   └── michi-multiroom/    # Multiroom audio (future)
```

## Design Principles

- **Separation of concerns**: Each crate has a single responsibility.
- **Extensibility**: Future features are isolated in their own crates.
- **Error handling**: All fallible operations return typed errors via `thiserror`.
- **Observability**: All internal logging uses `tracing`.
- **Configuration**: Environment-driven configuration with sensible defaults.

## Crate Descriptions

### michi-core
Contains all shared data types: `Track`, `Album`, `Artist`, `Playlist`, `PlaybackState`, `AudioFormat`, `AudioMetadata`. These models are designed to be compatible with Michi Music Player and Michi Mobile for future sync capabilities.

### michi-api
Axum-based HTTP router. Defines all endpoints and handlers. Receives shared state (Config).

### michi-config
Reads configuration from environment variables with defaults suitable for containerized deployment.

### michi-db
SQLite database layer using SQLx. Handles connection pooling and schema migrations.

### michi-metadata
Audio metadata extraction using the Lofty crate. Parses tags and audio properties from music files.

### michi-scanner
Recursively scans directories for audio files, reads metadata, and builds a track database.

### Placeholder Crates
`michi-streaming`, `michi-homeassistant`, `michi-sync`, and `michi-multiroom` are prepared for future development and currently export placeholder functions.

## Data Flow

```
Config (env) → main.rs → Router (michi-api)
                                  ↓
                           HTTP Request
                                  ↓
                           Handler → Response
```

When scanning:
```
Scanner → Metadata Reader → Database
```
