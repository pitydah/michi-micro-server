# Michi Micro Server Web UI Functional Matrix

Principio rector: **NO SUCCESS WITHOUT VERIFIED EFFECT**

## Clasificación de Estados
- `FUNCTIONAL`: Control con endpoint real, backend verificado, persistencia/runtime probado y efecto real confirmado.
- `PARTIAL`: Funciona parcialmente; algunos efectos secundarios no están completamente integrados.
- `NOT_IMPLEMENTED`: No implementado; la UI lo comunica honestamente sin simular éxito.
- `UNAVAILABLE`: Dependencia o componente fuera de línea / no disponible en el runtime actual.
- `RESTART_REQUIRED`: La acción surte efecto tras reiniciar el servidor.
- `BROKEN`: Control o flujo que presenta fallos o falsos positivos conocidos.

---

## Matriz de Controles y Acciones Web UI (UI-001..UI-030)

| Ref ID | UI Element / Control | JS Handler | API Endpoint | Backend Handler | Persistencia | Runtime Effect | Evidencia | Estado |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :---: |
| **UI-001** | QR Pairing / Generate | `generateQR()` | `POST /api/v1/pair/qr` | `qr_generate_handler` | SQLite (`pairing_qr_codes`) | Genera sesión y código QR con SVG estilizado | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-002** | QR Pairing / Claim Bootstrap | Mobile Client | `POST /api/v1/pair/qr/:code/claim` | `qr_claim_handler` | SQLite (`pairing_qr_codes`, `link_devices`) + Token Store | Reclamo unauthenticated, atómico, anti-replay, emisión de token | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-003** | QR Pairing / Status Poll | `pollQRStatus()` | `GET /api/v1/pair/qr/:code/status` | `qr_status_handler` | SQLite (`pairing_qr_codes`) | Detección real WAITING -> CLAIMED / EXPIRED | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-004** | Receivers / Online & Active Calculation | `loadReceivers()` | `GET /api/v1/receivers` | `receivers_handler` | Memoria (ReceiverRegistry) | Desacoplamiento de `online` (heartbeat < 180s) y `session_active` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-005** | Queue / Add Single Track (+Q) | `addToQueue(idx)` | `POST /api/v1/queue/items` | `queue_items_handler` | SQLite (`queues`, `queue_items`) | Validación de existencia y append transaccional en cola única activa | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-006** | Queue / Render Active Queue | `loadQueue()` | `GET /api/v1/queue` | `queue_handler` | SQLite (`queues`, `queue_items`) | Carga cola activa canónica con tracks resueltos | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-007** | Queue / Multi-track Append (+Q All) | `addAllToQueue()` | `POST /api/v1/queue/items` | `queue_items_handler` | SQLite (`queues`, `queue_items`) | Inserción atómica con validación completa y rollback | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-008** | Queue / Reorder Items | `reorderQueue(order)` | `PUT /api/v1/queue/reorder` | `queue_reorder_handler` | SQLite (`queue_items`) | Actualización transaccional de posiciones preservando track IDs | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-009** | Queue / Jump to Item | `jumpQueue(idx)` | `POST /api/v1/queue/jump` | `queue_jump_handler` | SQLite + `PlaybackState` | Selección canónica de track y reseteo de posición a 0 | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-010** | Queue / Delete Queue | `deleteQueue(id)` | `DELETE /api/v1/queue/:id` | `queue_delete_handler` | SQLite (`queues`, `queue_items`) | Eliminación transaccional con retorno 404 ante inexistencia | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-011** | Playback Chain / Zero Link Rejection | `playChain(id)` | `POST /api/v1/chains/:id/play` | `play_chain_handler` | Memoria / `PlaybackState` | Retorna HTTP 400 `NO_OUTPUTS` y mantiene `playing = false` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-012** | Playback Chain / Zero Active Failure | `playChain(id)` | `POST /api/v1/chains/:id/play` | `play_chain_handler` | SQLite + Receiver Sessions | Retorna HTTP 502 `PLAYBACK_FAILED` si fallan todos los links | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-013** | Playback Chain / Stop | `stopChain(id)` | `POST /api/v1/chains/:id/stop` | `stop_chain_handler` | Receiver Sessions + `PlaybackState` | Detiene sesiones en todos los receivers activos y reporta estado | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-014** | Playback Chain / Volume Control | `setChainVolume(id, v)` | `POST /api/v1/chains/:id/volume` | `chain_volume_handler` | SQLite (`chain_links`) + Hardware Volume | Validación estricta 0..=100 y reporte por receiver | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-015** | Room Group / Empty Rejection | `activateRoomGroup(id)`| `POST /api/v1/rooms/groups/:id/activate` | `activate_room_group_handler` | SQLite (`room_groups`) | Retorna HTTP 400 `INVALID_ROOM` si no tiene receivers | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-016** | Room Group / SQLite Persistence | `createRoomGroup()` | `POST /api/v1/rooms/groups` | `create_room_group_handler` | SQLite (`room_groups`) | Persistencia durable de id, name, mode, receiver_ids, volumes | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-017** | Rooms / List & Create | `loadRooms()`, `createRoom()` | `GET /api/v1/rooms`, `POST /api/v1/rooms` | `rooms_handler`, `create_room_handler` | SQLite (`room_groups`) | Lista y crea salas persistentes verificando existencia de receivers | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-018** | Rooms / Play in Room | `playInRoom(id, track)`| `POST /api/v1/rooms/:id/play` | `room_play_handler` | SQLite + Receiver Sessions | Inicia sesiones PCM s16le 48k/16/2 y verifica éxito antes de playing | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-019** | Settings / Effective Metrics | `loadSettings()` | `GET /api/v1/settings` | `get_settings_handler` | Config + Runtime Telemetry | Expone scan_workers, transcode_workers, db_pool efectivos | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-020** | Settings / Persistent Restart Banner | `saveSetting(k, v)` | `PUT /api/v1/settings` | `update_settings_handler` | Archivo Config (`config.json`) | Persiste a disco y mantiene banner persistente `⚠ Restart required` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` / `RESTART_REQUIRED` |
| **UI-021** | Handoff / Transfer State | `transferHandoff()` | `POST /api/v1/player/handoff` | `handoff_handler` | `PlaybackState` | Transfiere estado canónico y actualiza snapshot | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-022** | Broadcast / Browser Output Wording | `playSource(id)` | Local `<audio>` / Stream Proxy | `stream_proxy_handler` | N/A | Comunica explícitamente `(This Browser)` y salida local | `crates/michi-api/static/app.js` | `FUNCTIONAL` |
| **UI-023** | Podcast / Played State Guard | `playEpisode(id)` | `PUT /api/v1/sources/episodes/:id` | `update_episode_handler` | SQLite (`podcast_episodes`) | Marca `played: true` únicamente tras éxito real de `audio.play()` | `crates/michi-api/static/app.js` | `FUNCTIONAL` |
| **UI-024** | Podcast / Feed Ingestion Status | `addSource(url)` | `POST /api/v1/sources` | `add_source_handler` | SQLite (`stream_sources`, `podcast_episodes`) | Reporta feed_status, episodes_imported y episodes_failed | `crates/michi-api/src/routes/v1/sources.rs` | `FUNCTIONAL` |
| **UI-025** | Top Bar / Library Scan | `handleScan()` | `POST /api/library/scan` | `library_scan_handler` | SQLite (`tracks`) | Escaneo de directorios con upsert y reporte de tracks indexados | `crates/michi-api/src/routes/v1/library.rs` | `FUNCTIONAL` |
| **UI-026** | Michi Link / Connectivity Test | `testMichiLink()` | Multi-probe async (`/health/live`, `/api/v1/server/info`) | Core Endpoints | Memoria | Verificación multi-probe real (Testing, Connected, Degraded, Failed) | `crates/michi-api/static/app.js` | `FUNCTIONAL` |
| **UI-027** | Webhook / Reachability Test | `testWebhook()` | `POST /api/v1/webhook/test` | `test_webhook_handler` | Red HTTP externa | Validación HTTP real con código de estado y latencia de respuesta | `crates/michi-api/src/routes/v1/settings.rs` | `FUNCTIONAL` |
| **UI-028** | Backup / Verify Integrity | `verifyBackup()` | `POST /api/v1/backup/verify` | `verify_integrity_handler` | Filesystem | Verificación de existencia de archivos en disco (Available / Missing) | `crates/michi-api/src/routes/v1/backup.rs` | `FUNCTIONAL` |
| **UI-029** | Endpoint Drift Verification | Automated CI Gate | Router route introspection | Axum Router | N/A | Verificación automatizada de consistencia entre UI JS y rutas Axum | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **UI-030** | Default Port 9090 Contract | Automated CI Gate | Config & Deploy files | Config / Systemd / Docker | N/A | Puerto 9090 unificado en toda la base de código | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |

---

## Matriz de Semántica de Reproducción Canónica (PLAY-01..PLAY-12)

| Ref ID | Semántica | Endpoint / Comando | Backend Behavior | Evidencia | Estado |
| :--- | :--- | :--- | :--- | :--- | :---: |
| **PLAY-01** | Track Selection | `POST /api/v1/playback/control` (`play`, `track_id`) | Actualiza `track_id`, reinicia `position_ms = 0`, `playing = true` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-02** | Pause | `POST /api/v1/playback/control` (`pause`) | Establece `playing = false`, preserva `position_ms` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-03** | Resume | `POST /api/v1/playback/control` (`play`) | Reanuda desde `position_ms` actual, `playing = true` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-04** | Seek | `POST /api/v1/playback/control` (`seek`, `position_ms`) | Actualiza `position_ms` con timestamp exacto | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-05** | Strict Volume | `POST /api/v1/playback/control` (`set_volume`) | Valida estrictamente `0..=100`, rechaza valores fuera de rango | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-06** | Repeat None | `POST /api/v1/playback/control` (`next` al fin de cola) | Detiene reproducción al terminar la cola (`playing = false`) | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-07** | Repeat All | `POST /api/v1/playback/control` (`next` al fin de cola) | Vuelve al primer track de la cola activa (`position_ms = 0`) | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-08** | Repeat One | `POST /api/v1/playback/control` (`next`) | Reinicia el track actual en bucle (`position_ms = 0`) | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-09** | Next Traversal | `POST /api/v1/playback/control` (`next`) | Recorre ítems ordenados por `position` en la cola única activa | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-10** | Previous Navigation | `POST /api/v1/playback/control` (`previous`) | Si `pos > 3s` reinicia track actual; si no, navega al track previo | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-11** | Shuffle Navigation | `POST /api/v1/playback/control` (`shuffle: true`) | Navega por la cola de forma pseudoaleatoria/barajada | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
| **PLAY-12** | Output Target Truth | `GET /api/v1/playback/state` | Informa `Output: This Browser` vs `Michi Stream / Rooms` | `tests/web_ui_integrity_tests.rs` | `FUNCTIONAL` |
