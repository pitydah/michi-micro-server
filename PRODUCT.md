# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- **Primary — the self-hosting home music owner.** Runs Michi Micro Server on a Raspberry Pi, NAS, or mini PC (CasaOS/ZimaOS app store, Docker, or systemd/Debian). Their job in the web UI: listen to their own library, and occasionally administer the server. They reach the UI from a desktop browser or as an installed PWA, over LAN or Tailscale.
- **Secondary — the Michi ecosystem.** Michi Music Player, Michi Mobile (planned), and Home Assistant pair with the server through the Michi Link contract. They consume the API, but the web UI is where pairing, QR codes, and device visibility live.

## Product Purpose

A self-hosted, lightweight home music server written in Rust: it centralizes the local library, streams it over the home network, manages playlists, drives multi-room playback, and syncs across the Michi ecosystem. The web UI is its control surface — a place to listen first and administer second.

## Positioning

The claim a neighboring product could not truthfully copy: a complete open music ecosystem — Michi Link contract v1, OpenSubsonic-compatible layer, playback chains to multiple receivers, room groups, multi-server sync — that runs within a <50 MB idle RAM budget on a Raspberry Pi. "Micro" is a product promise (weightless execution), not a limitation; the ecosystem is the story, and the micro footprint is how it feels.

## Operating Context

- **Deployment:** Docker multi-arch (`linux/amd64` + `linux/arm64`), CasaOS/ZimaOS app store, systemd + Debian package. Serves on port 8096 without TLS — designed to run behind a reverse proxy, not exposed to the internet.
- **Use:** LAN or Tailscale; desktop browser and installed PWA (standalone, offline-track caching via IndexedDB); audio playback through the HTML `<audio>` element; control from a phone browser works too.
- **Workflows:** scan library, browse and search, play, route playback chains to receivers, QR-pair with Michi Mobile, review play history, upload files, create backups/snapshots, configure webhooks.
- **Resource profiles:** Eco / Balanced / Performance, user-selectable.
- **UI languages:** 9 (en, es, pt, de, fr, it, ru, zh, ja). Documentation is in English.

## Capabilities and Constraints

**Confirmed capabilities**
- Library scanning and indexing; full-text search with field filters.
- Streaming with HTTP Range; transcoding to MP3/Ogg/HLS.
- Playlists: CRUD, smart playlists (8 rules), M3U import/export, backup export.
- Playback chains to multiple receivers with per-device volume; room groups; broadcast/cast sources (radio, podcast RSS, HLS).
- Play history with stats and export.
- Michi Link pairing v1 (QR + 6-digit codes), feature negotiation, ecosystem device list.
- Receiver discovery (mDNS), sessions, groups; multi-server sync and WebSocket handoff; resumable upload with SHA-256 dedup.
- Backup/snapshot, integrity check, webhook, OpenSubsonic layer (partial: 5 endpoints, JSON-only), bearer-token auth, rate limiting.

**Constraints**
- Idle RAM target <50 MB; Docker image <100–200 MB; low-power ARM devices.
- Single-file SQLite persistence; per-machine Ed25519 identity.
- Formats: mp3, flac, ogg, opus, aac, m4a, wav. AIFF/DSF/DFF are explicitly excluded — "use Michi Big Server for those formats."

**Undecided / beta**
- Receivers and rooms are feature-gated off (`features.receivers=false`, `features.rooms=false`) pending the "Michi Music Stream" hardware; the UI must render them gracefully as future/unavailable.
- Offline sync is not implemented; Michi Mobile is planned but does not exist yet.

## Brand Commitments

- **Name:** Michi Micro Server; short form "Michi".
- **Taglines:** "Lightweight, robust home music server written in Rust." · "Stream your local music library over your home network or Tailscale."
- **Mascot:** the orange tabby cat wearing headphones — confirmed brand personality by the owner. It must survive the redesign, re-imagined or not; it belongs in moments of rest (empty states, pairing, welcome), not in the daily operational path.
- **Assets:** `static/assets/michi-logo.svg` (headphone/play glyph, violet on deep navy), `static/assets/michi-micro-server.svg`/`.png` (favicon), `static/assets/michi-micro-server-512.png` (official app-store/PWA icon), and `static/assets/michi-hero-cat.webp` (hero cat). No formal brand guide exists.
- **Voice:** technical, sober, understated; "micro" is communicated through execution — speed, calm, low friction — not through boastful copy.

## Evidence on Hand

- 20 repository docs (architecture, API, Michi Link v1 spec, client integration spec, OpenSubsonic compatibility, CasaOS/ZimaOS, receiver integration plan, implementation evidence, master checklist); 168+ passing tests; Docker multi-arch images published on ghcr.io; systemd unit and Debian packaging.
- **Documented absences — do not fabricate:** no UI screenshots anywhere, no marketing landing page (the root serves the control SPA), no brand guide, no measured resource numbers (RESOURCE_BUDGET targets are unverified), and no evidence about Michi Big Server beyond the format-exclusion note.

## Product Principles

1. **Ecosystem story, micro soul.** The UI presents the hub of a music ecosystem; weightlessness (speed, calm, low friction) is how "micro" shows up — never cheap, never toy-like.
2. **Listen first, administer behind.** Browsing and playing are the front door; technical control exists, folds away, and is never loud.
3. **Local-first trust.** The UI must feel private, personal, self-owned — no cloud theater, no SaaS patterns.
4. **Personality without noise.** The cat and the warmth live in moments of rest (empty states, pairing, welcome), never in the operational path.
5. **One voice at every scale.** The same visual language serves the desktop console and the phone PWA, dashboard and deep settings.

## Accessibility & Inclusion

- The incumbent UI already honors `prefers-reduced-motion`, visible focus rings, a skip link, and keyboard-operable navigation; the redesign must keep this baseline.
- 9-language i18n is a maintained product requirement, not a stretch goal.
