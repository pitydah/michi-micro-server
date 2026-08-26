# Michi Micro Server — WebUI Functional Conformance Specification

## 1. Objective & Architectural Guarantees

This document defines the functional-conformance standard for the Michi Micro Server WebUI.
The WebUI is an auditable, truthful control surface over real backend services and SQLite database state.

### Non-Negotiable Core Invariants:
1. **Canonical Contract (`/api/v1`)**: All WebUI interactions must target canonical `/api/v1` routes where defined. Legacy unversioned `/api/` routes are strictly prohibited.
2. **Browser Authentication**: Browser sessions use `michi_web_session` (`HttpOnly`, `SameSite=Strict`, `Path=/`, and `Secure` when served over TLS) cookies alongside `Authorization: Bearer <token>` for standalone clients. No tokens stored in `localStorage` or DOM.
3. **Fail-Safe Filesystem Access**: Under no circumstances may application management actions modify or recursively chown `/music` or configured music library paths.
4. **State Truthfulness**: `UNKNOWN` must remain `UNKNOWN`. No UI indicator may display fake "Online" or mock data unless verified by the server.
5. **Clean HTTP Requests**: Automatic JSON payload typing and empty `{}` request bodies on `POST`/`PUT`/`PATCH` to prevent `415 Unsupported Media Type` failures.

---

## 2. Functional Domains & Endpoint Catalog

| Domain | Function ID | Method | Endpoint | UI Action | Maturity | Status |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Status** | `server.status` | GET | `/api/v1/status` | `loadServerStatus` | Stable | `wired` |
| **Server Info** | `server.info` | GET | `/api/v1/server/info` | `loadServerInfo` | Stable | `wired` |
| **Capabilities** | `server.capabilities` | GET | `/api/v1/capabilities` | `loadCapabilities` | Stable | `wired` |
| **Auth** | `auth.check` | GET | `/api/auth/check` | `checkAuthSession` | Stable | `wired` |
| **Auth** | `auth.login` | POST | `/api/auth/login` | `handleLogin` | Stable | `wired` |
| **Auth** | `auth.register` | POST | `/api/auth/register` | `handleRegister` | Stable | `wired` |
| **Auth** | `auth.logout` | POST | `/api/auth/logout` | `handleLogout` | Stable | `wired` |
| **Dashboard** | `home.dashboard` | GET | `/api/v1/home/dashboard` | `loadDashboard` | Stable | `wired` |
| **Library** | `library.stats` | GET | `/api/v1/library/stats` | `loadLibraryStats` | Stable | `wired` |
| **Library** | `library.tracks` | GET | `/api/v1/tracks` | `loadTracks` | Stable | `wired` |
| **Library** | `library.track_detail` | GET | `/api/v1/tracks/:id` | `getTrackDetail` | Stable | `wired` |
| **Search** | `library.search` | GET | `/api/v1/search` | `handleSearch` | Stable | `wired` |
| **Search** | `library.search_advanced` | GET | `/api/v1/search/advanced` | `handleAdvancedSearch` | Stable | `wired` |
| **Scanning** | `library.scan` | POST | `/api/v1/library/scan` | `handleScan` | Stable | `wired` |
| **Artwork** | `library.artwork` | GET | `/api/v1/artwork/:id` | `renderArtwork` | Stable | `wired` |
| **Streaming** | `library.stream` | GET | `/api/v1/stream/:id` | `playStream` | Stable | `wired` |
| **Download** | `library.download` | GET | `/api/v1/download/:id` | `downloadTrack` | Stable | `wired` |
| **Playlists** | `playlists.list` | GET | `/api/v1/playlists` | `loadPlaylists` | Stable | `wired` |
| **Playlists** | `playlists.create` | POST | `/api/v1/playlists` | `createPlaylist` | Stable | `wired` |
| **Playlists** | `playlists.get` | GET | `/api/v1/playlists/:id` | `openPlaylist` | Stable | `wired` |
| **Playlists** | `playlists.update` | PUT | `/api/v1/playlists/:id` | `renamePlaylist` | Stable | `wired` |
| **Playlists** | `playlists.delete` | DELETE | `/api/v1/playlists/:id` | `deletePlaylist` | Stable | `wired` |
| **Playlists** | `playlists.tracks` | GET | `/api/v1/playlists/:id/tracks` | `loadPlaylistTracks` | Stable | `wired` |
| **Playlists** | `playlists.add_track` | POST | `/api/v1/playlists/:pid/tracks/:tid` | `addTrackToPlaylist` | Stable | `wired` |
| **Playlists** | `playlists.remove_track`| DELETE | `/api/v1/playlists/:pid/tracks/:tid` | `removeTrackFromPlaylist` | Stable | `wired` |
| **Playlists** | `playlists.reorder` | PUT | `/api/v1/playlists/:id/reorder` | `reorderPlaylistTracks` | Stable | `wired` |
| **Favorites** | `library.favorites_list`| GET | `/api/v1/starred` | `loadFavorites` | Stable | `wired` |
| **Favorites** | `library.star_toggle` | POST | `/api/v1/star/:id` | `toggleFavorite` | Stable | `wired` |
| **Ratings** | `library.rate` | POST | `/api/v1/rate/:id` | `setRating` | Stable | `wired` |
| **Bookmarks** | `library.bookmarks_list`| GET | `/api/v1/bookmarks` | `loadBookmarks` | Stable | `wired` |
| **Bookmarks** | `library.bookmark_create`| POST | `/api/v1/bookmarks` | `createBookmark` | Stable | `wired` |
| **Duplicates** | `library.duplicates` | GET | `/api/v1/library/duplicates` | `loadDuplicates` | Stable | `wired` |
| **Insights** | `library.artist_insights`| GET | `/api/v1/artists/:name/insights` | `loadArtistInsights` | Stable | `wired` |
| **Insights** | `library.album_health` | GET | `/api/v1/albums/:key/health` | `loadAlbumHealth` | Stable | `wired` |
| **Queue** | `queue.get` | GET | `/api/v1/queue` | `loadQueue` | Stable | `wired` |
| **Queue** | `queue.add_items` | POST | `/api/v1/queue/items` | `addToQueue` | Stable | `wired` |
| **Queue** | `queue.jump` | POST | `/api/v1/queue/jump` | `jumpQueueItem` | Stable | `wired` |
| **Queue** | `queue.reorder` | PUT | `/api/v1/queue/reorder` | `reorderQueue` | Stable | `wired` |
| **Queue** | `queue.save` | POST | `/api/v1/queue/save` | `saveQueueAsPlaylist` | Stable | `wired` |
| **Queue** | `queue.saved_list` | GET | `/api/v1/queue/saved` | `loadSavedQueues` | Stable | `wired` |
| **Playback** | `playback.state` | GET | `/api/v1/playback/state` | `loadPlaybackState` | Stable | `wired` |
| **Playback** | `playback.control` | POST | `/api/v1/playback/control` | `sendPlaybackControl` | Stable | `wired` |
| **History** | `history.list` | GET | `/api/v1/history` | `loadHistory` | Stable | `wired` |
| **History** | `history.stats` | GET | `/api/v1/history/stats` | `loadHistoryStats` | Stable | `wired` |
| **History** | `history.export` | GET | `/api/v1/history/export` | `exportHistory` | Stable | `wired` |
| **History** | `history.clear` | DELETE | `/api/v1/history` | `clearHistory` | Stable | `wired` |
| **Import** | `import.preflight` | POST | `/api/v1/import/preflight` | `runImportPreflight` | Stable | `wired` |
| **Import** | `import.session_create`| POST | `/api/v1/import/session` | `createImportSession` | Stable | `wired` |
| **Import** | `import.upload` | POST | `/api/v1/import/upload/:id` | `uploadImportTrack` | Stable | `wired` |
| **Import** | `import.status` | GET | `/api/v1/import/session/:id` | `pollImportSession` | Stable | `wired` |
| **Import** | `import.commit` | POST | `/api/v1/import/commit/:id` | `commitImportSession` | Stable | `wired` |
| **Import** | `import.rollback` | POST | `/api/v1/import/rollback/:id` | `rollbackImportSession` | Stable | `wired` |
| **Pairing** | `link.qr_start` | POST | `/api/v1/pair/qr` | `generateQrPairing` | Stable | `wired` |
| **Devices** | `link.devices_list` | GET | `/api/v1/link/devices` | `loadLinkedDevices` | Stable | `wired` |
| **Devices** | `link.device_revoke`| DELETE | `/api/v1/link/devices/:id` | `revokeLinkedDevice` | Stable | `wired` |
| **Receivers** | `receivers.list` | GET | `/api/v1/receivers` | `loadReceivers` | Stable | `wired` |
| **Receivers** | `receivers.discover` | POST | `/api/v1/devices/discover` | `discoverReceivers` | Stable | `wired` |
| **Receivers** | `receivers.pair_start` | POST | `/api/v1/receivers/pair/start` | `startReceiverPairing` | Stable | `wired` |
| **Receivers** | `receivers.pair_confirm`| POST | `/api/v1/receivers/pair/confirm`| `confirmReceiverPairing`| Stable | `wired` |
| **Rooms** | `rooms.list` | GET | `/api/v1/rooms/groups` | `loadRooms` | Stable | `wired` |
| **Rooms** | `rooms.create` | POST | `/api/v1/rooms/groups` | `createRoomGroup` | Stable | `wired` |
| **Rooms** | `rooms.activate` | POST | `/api/v1/rooms/groups/:id/activate` | `activateRoomGroup` | Stable | `wired` |
| **Rooms** | `rooms.deactivate` | POST | `/api/v1/rooms/groups/:id/deactivate` | `deactivateRoomGroup` | Stable | `wired` |
| **Rooms** | `rooms.play` | POST | `/api/v1/rooms/:id/play` | `playRoomTrack` | Stable | `wired` |
| **Chains** | `chains.list` | GET | `/api/v1/chains` | `loadPlaybackChains` | Stable | `wired` |
| **Chains** | `chains.play` | POST | `/api/v1/chains/:id/play` | `playPlaybackChain` | Stable | `wired` |
| **Sources** | `sources.list` | GET | `/api/v1/sources` | `loadSources` | Stable | `wired` |
| **Webhooks** | `webhook.get` | GET | `/api/v1/webhook` | `loadWebhookConfig` | Stable | `wired` |
| **Webhooks** | `webhook.test` | POST | `/api/v1/webhook/test` | `testWebhook` | Stable | `wired` |
| **Backup** | `backup.create` | POST | `/api/v1/backup/snapshot` | `createBackupSnapshot` | Stable | `wired` |
| **Backup** | `backup.download` | GET | `/api/v1/backup/download` | `downloadBackupArchive` | Stable | `wired` |
| **Backup** | `backup.restore` | POST | `/api/v1/backup/restore` | `restoreBackupArchive` | Stable | `wired` |
| **Diagnostics** | `diagnostics.report` | GET | `/api/v1/diagnostics` | `loadDiagnosticsReport` | Stable | `wired` |
| **Self Test** | `health.selftest` | GET | `/api/v1/health/self-test` | `runSelfTest` | Stable | `wired` |
| **Mounts** | `health.mounts` | GET | `/api/v1/health/mounts` | `loadMountHealth` | Stable | `wired` |
| **Storage** | `health.storage` | GET | `/api/v1/health/storage` | `loadStorageHealth` | Stable | `wired` |
| **Config** | `config.validate` | GET | `/api/v1/config/validate` | `loadConfigValidation` | Stable | `wired` |
| **Jobs** | `jobs.list` | GET | `/api/v1/jobs` | `loadJobs` | Stable | `wired` |
| **Jobs** | `jobs.cancel` | POST | `/api/v1/jobs/:id/cancel` | `cancelJob` | Stable | `wired` |
| **Modules** | `modules.list` | GET | `/api/v1/modules` | `loadModules` | Stable | `wired` |
| **Modules** | `modules.toggle` | POST | `/api/v1/modules/:name` | `toggleModule` | Stable | `wired` |
| **Audit** | `audit.log` | GET | `/api/v1/audit/log` | `loadAuditLog` | Stable | `wired` |
| **Changes** | `changes.journal` | GET | `/api/v1/changes` | `loadChangeJournal` | Stable | `wired` |
| **Settings** | `settings.get` | GET | `/api/v1/settings` | `loadSettings` | Stable | `wired` |
| **Settings** | `settings.update` | POST | `/api/v1/settings` | `saveSettings` | Stable | `wired` |
| **Events** | `events.stream` | GET | `/api/v1/events` | `connectLiveEvents` | Stable | `wired` |
| **Sync** | `sync.capabilities` | GET | `/api/v1/sync/capabilities` | `loadSyncCapabilities` | Stable | `wired` |
| **Sync** | `sync.devices` | GET | `/api/v1/sync/devices` | `loadSyncDevices` | Stable | `wired` |

