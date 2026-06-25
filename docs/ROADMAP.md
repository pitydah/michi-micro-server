# Roadmap

## Phase 1: Server Base
- [x] Rust workspace setup
- [x] HTTP server with Axum
- [x] Core models (Track, Album, Artist, etc.)
- [x] Configuration from environment
- [x] Health check endpoints
- [ ] Unit tests for core models

## Phase 2: Scanner + SQLite
- [x] SQLite database layer
- [x] Metadata reading with Lofty
- [x] Directory scanner
- [ ] CRUD endpoints for library
- [ ] Database migrations
- [ ] Library management API

## Phase 3: Streaming
- [ ] Audio streaming endpoint
- [ ] Range request support
- [ ] Transcoding via FFmpeg
- [ ] Cover art serving
- [ ] HLS or direct streaming

## Phase 4: Web Interface
- [ ] Basic web UI
- [ ] Library browser
- [ ] Now playing view
- [ ] Search functionality
- [ ] Playlist management

## Phase 5: Home Assistant
- [ ] MQTT client setup
- [ ] MQTT Discovery integration
- [ ] media_player entity
- [ ] Sensors and controls
- [ ] Auto-discovery

## Phase 6: CasaOS / ZimaOS
- [ ] App store submission
- [ ] Icon and screenshots
- [ ] CasaOS dashboard widget
- [ ] One-click install

## Phase 7: Synchronization
- [ ] Sync API for Michi Music Player
- [ ] Sync API for Michi Mobile
- [ ] Conflict resolution
- [ ] Offline support

## Phase 8: Multiroom
- [ ] Snapcast/Snapserver integration
- [ ] Multi-room sync
- [ ] Group management
- [ ] Latency compensation
