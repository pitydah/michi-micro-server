# Michi Micro Server Web UI Functional Matrix

Principio rector: **NO SUCCESS WITHOUT VERIFIED EFFECT**

## Clasificación de Estados
- `FUNCTIONAL`: Control con endpoint real, backend verificado, persistencia/runtime probado y ACK honesto.
- `PARTIAL`: Funciona parcialmente; algunos efectos secundarios no están completamente integrados.
- `NOT_IMPLEMENTED`: No implementado; la UI lo comunica honestamente sin simular éxito.
- `UNAVAILABLE`: Dependencia o componente fuera de línea / no disponible en el runtime actual.
- `RESTART_REQUIRED`: La acción surte efecto tras reiniciar el servidor.
- `BROKEN`: Control o flujo que presenta fallos o falsos positivos conocidos.

---

## Matriz de Controles y Acciones Web UI

| UI Element / Control | JS Handler | API Endpoint | Backend Handler | Persistencia | Runtime Effect | Evidencia | Estado |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :---: |
| **Top Bar / Scan** | `handleScan()` | `POST /api/library/scan` | `library_scan_handler` | SQLite (`tracks`) | Escaneo directorios y upsert en DB | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Michi Link / Test** | `testMichiLink()` | Probes async (`/health/live`, `/api/v1/server/info`, `/api/v1/capabilities`) | `health_handler`, `server_info_handler`, `capabilities_handler` | Memoria | Verificación real multi-probe con estados TESTING/CONNECTED/DEGRADED/FAILED | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Server URL / Copy** | `copyServerUrl()` | `window.location.origin` / Host header | N/A (Frontend + Host derivation) | Ephemeral UI | Copia URL accesible en red local | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **QR Pairing / Generate** | `generateQR()` | `POST /api/v1/pair/qr` | `qr_generate_handler` | SQLite (`pairing_qr_codes`) | Genera código QR con URL accesible | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **QR Pairing / Status Poll** | `pollQRStatus()` | `GET /api/v1/pair/qr/:code/status` | `qr_status_handler` | SQLite (`pairing_qr_codes`) | Detección real WAITING -> CLAIMED / EXPIRED | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **QR Pairing / Claim** | Client claim | `POST /api/v1/pair/qr/:code/claim` | `qr_claim_handler` | Token store + `michi_link_devices` | Registra token y dispositivo en ecosistema | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Receivers / Scan Network** | `scanReceivers()` | `POST /api/v1/receivers/discover` | `discover_receivers_handler` | Memoria | mDNS browse `_michi-link._tcp.local.` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Library / Play Track** | `playTrack(idx)` | `POST /api/v1/playback/control` | `playback_control_handler` | `PlaybackState` | Inicia reproducción canónica en Micro Server | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Player / Play-Pause** | `playPause()` | `POST /api/v1/playback/control` | `playback_control_handler` | `PlaybackState` | Toggle canónico play/pause en Micro Server | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Player / Seek** | `seekTo(pos)` | `POST /api/v1/playback/control` | `playback_control_handler` | `PlaybackState` | Seek canónico de posición | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Player / Volume** | `setVolume(vol)` | `POST /api/v1/playback/control` | `playback_control_handler` | `PlaybackState` | Ajuste canónico de volumen | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Player / Repeat & Shuffle** | `toggleRepeat()`, `toggleShuffle()` | `POST /api/v1/playback/control` | `playback_control_handler` | `PlaybackState` | Persistencia y propagación en `PlaybackState` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Queue / Add Track** | `addToQueue(idx)` | `POST /api/v1/queue/items` | `queue_items_handler` | SQLite (`queues`, `queue_items`) | Inserción en cola canónica persistente | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Queue / Render** | `loadQueue()` | `GET /api/v1/queue` / `GET /api/v1/queue/saved` | `queue_handler`, `queue_saved_handler` | SQLite | Carga cola canónica del servidor | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Room Group / Activate** | `activateRoomGroup(id)` | `POST /api/v1/rooms/:id/activate` | `activate_room_group_handler` | SQLite + Receiver sessions | Sesiones `pcm_s16le/48k/16/2` con reporte estructurado por receiver | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Playback Chain / Play** | `playChain(id)` | `POST /api/v1/chains/:id/play` | `play_chain_handler` | SQLite + Receiver sessions | Sesiones `pcm_s16le/48k/16/2` y conteo verificado de `links_active` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Playback Chain / Volume** | `setChainVolume(id, vol)` | `PUT /api/v1/chains/:id/master-volume` | `set_chain_master_volume_handler` | SQLite + Receiver sessions | Debounced request con rollback ante error | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Settings / Save** | `saveSetting(k, v)` | `PUT /api/v1/settings` | `update_settings_handler` | Archivo Config (`config.json`) | Guarda configuración y notifica `restart_required` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` / `RESTART_REQUIRED` |
| **Settings / Webhook Test** | `testWebhook()` | `POST /api/v1/webhook/test` | `test_webhook_handler` | Red HTTP real | POST HTTP real; éxito solo con HTTP 2xx y latencia reportada | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Library / File Availability**| `verifyIntegrity()` | `POST /api/v1/backup/verify` | `verify_integrity_handler` | Filesystem | Comprueba existencia en disco (Available / Missing) | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Library / Create Snapshot** | `createSnapshot()` | `POST /api/v1/backup/snapshot` | `snapshot_handler` | SQLite (`snapshots`) | Almacena estadísticas del catálogo | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Library / Watcher Status** | Render Settings | `GET /api/v1/settings` | `get_settings_handler` | N/A | Comunica "Manual Scan Only" (sin falso Active) | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **Handoff / Transfer State** | `transferState()` | `POST /api/v1/queue/transfer` | `queue_transfer_handler` | SQLite + `PlaybackState` | Transfiere estado de reproducción canónico | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
