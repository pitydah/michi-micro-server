# Discovery & Provisional Trust Rules (R1.4)

## 1. Discovery Transports Advertised by Michi Micro Server

1. **Whisker UDP Multicast**:
   - Group: `224.0.0.167`, Port: `53318`
   - Payload: JSON matching `vendor/michi-link/schemas/discovery-announce.schema.json`
   - Signed with the server's persistent Ed25519 key (`michi_id`, `public_key`, `signature`, `michi_home_id`, `host`, `port`, `roles`, `features`).
2. **mDNS / DNS-SD**:
   - Service type: `_michi-link._tcp.local.`
   - TXT records: `identity_scheme=ed25519-blake3-v1`, `michi_id`, `public_key`, `api_version=v1`, `service=michi-micro-server`, `version`, `michi_home_id`.
3. **Manual URL / Trusted QR**:
   - Direct LAN IP/hostname entry or scanning `AirCatTrustedEnrollmentQr` (`michi.aircat.home.enrollment.v1`).

## 2. Provisional-Until-Verified Rule (CRITICAL)

- Any LAN announcement (UDP multicast or mDNS) is **STRICTLY PROVISIONAL**.
- Mobile MUST NEVER transmit bearer tokens, admin credentials, or Home auth material to a discovered IP until:
  1. Ed25519 signature on `Announce` verifies (for Whisker) AND `BLAKE3(public_key) == michi_id`.
  2. `GET /api/v1/server/info` returns matching `michi_id`, `public_key`, `identity_scheme == "ed25519-blake3-v1"`, and `michi_home_id`.
  3. For Home/AirCat operations, connection switches to `pinnedBaseUrl` verified with X.509 + SPKI pin (`tlsSpkiSha256`).

## 3. Android Implementation Caveats (Wi-Fi / NSD / Docker)

- **UDP Multicast**: Acquire `WifiManager.MulticastLock` ONLY while the discovery screen is active (release in `onStop()` to preserve battery); verify `CHANGE_WIFI_MULTICAST_STATE` permission in `AndroidManifest.xml`.
- **mDNS / NSD**: Android `NsdManager` cannot run concurrent `resolveService` calls reliably on older API levels — queue resolution serially or use `resolveService` callback serialization.
- **Docker Bridge vs Host Network**: On ZimaOS/CasaOS Docker deployments, UDP multicast & mDNS require `network_mode: host` (or explicit `MICHI_ADVERTISE_HOST=<lan_ip>` with bridged port publishing + manual URL/QR fallback when multicast is blocked by AP isolation).
- **QR Origin Security (R1.4 §3.4)**: Legacy QR (`POST /api/v1/pair/qr`) strictly authorizes ONLY this server's real NIC IPs, real advertised `.local` hostnames, loopback, and explicit operator allowlists (`MICHI_PUBLIC_URL`, `MICHI_ALLOWED_QR_HOSTS`, `MICHI_ADVERTISE_HOST`, `MICHI_TAILSCALE_HOST`). Foreign LAN IPs and unrelated `.local`/`.ts.net` hosts are rejected with `400`.
