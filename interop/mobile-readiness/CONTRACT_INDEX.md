# Contract Index (R1.4 Frozen)

All schemas and signed golden vectors below live in the pinned `vendor/michi-link` submodule (`8dad0fb51128df98422bd74968e75f0ae8f85ccf`). **Do not duplicate or re-sign vectors** — consume directly from `vendor/michi-link`.

## 1. Normative Contract Document

- `vendor/michi-link/docs/MICHI_INTEROP_CONTRACT.md` (R1.4 normative rules, versioning policy, volume grammar, Home Roster v1, queue coordinator, QR origin policy).

## 2. Authoritative JSON Schemas (`vendor/michi-link/schemas/`)

| Domain | Schema File | Live Server Endpoint |
| --- | --- | --- |
| Server Identity & Features | `server-info.schema.json` | `GET /api/v1/server/info` |
| Discovery Announce | `discovery-announce.schema.json` | UDP `224.0.0.167:53318` |
| Home Identity | `michi-home.schema.json` | `GET /api/v1/home/info` |
| Home Membership | `michi-membership.schema.json` | Enrollment status / Auth / Roster |
| Home Device Roster (v1) | `home-device-roster.schema.json` | `GET /api/v1/home/roster` |
| Home Revocation | `home-device-revocation.schema.json` | `GET /api/v1/home/revocations`, `POST /api/v1/home/revoke` |
| Enrollment Challenge Req/Resp | `home-enrollment-challenge-request.schema.json`, `home-enrollment-challenge-response.schema.json` | `POST /api/v1/home/enrollment/challenge` |
| Enrollment Request | `home-enrollment-request.schema.json` | `POST /api/v1/home/enrollment/request` |
| Enrollment Status | `home-enrollment-status-response.schema.json` | `GET /api/v1/home/enrollment/{request_id}/status` |
| Device Auth Challenge Req/Resp | `device-auth-challenge-request.schema.json`, `device-auth-challenge-response.schema.json` | `POST /api/v1/auth/challenge` |
| Device Auth Session Req/Resp | `device-auth-session-request.schema.json`, `device-auth-session-response.schema.json` | `POST /api/v1/auth/session` |
| AirCat TLS Status | `aircat-tls-status.schema.json` | `GET /api/v1/aircat/status` |
| AirCat Enrollment QR | `aircat-enrollment-qr.schema.json` | `GET /api/v1/aircat/enrollment-qr` |
| AirCat Manifest | `aircat-manifest.schema.json` | `GET /api/v1/aircat/manifest` |
| AirCat Grant Request | `aircat-grant-request.schema.json` | `POST /api/v1/aircat/authorize` |
| AirCat Signed Grant | `aircat-signed-grant.schema.json` | `POST /api/v1/aircat/authorize` response & `X-Michi-AirCat-Grant` |
| Playback Control (Strict Vol) | `playback-control.schema.json` | `POST /api/v1/playback/control` |
| Playback State | `playback-state.schema.json` | `GET /api/v1/playback/state` |
| Queue DTOs | `queue.schema.json`, `queue-add-request.schema.json`, `queue-jump-request.schema.json`, `queue-transfer.schema.json` | `/api/v1/queue*` |
| Legacy Pairing | `pair-start.schema.json`, `pair-start-response.schema.json`, `pair-confirm.schema.json`, `pair-confirm-response.schema.json`, `qr-pairing.schema.json` | `/api/v1/pair/*` |
| Error Envelope | `error.schema.json` | All 4xx/5xx API responses |

## 3. Canonical Golden Vectors (`vendor/michi-link/tests/vectors/`)

- `tests/vectors/interop/micro-mobile-interop-vectors.json` — Ed25519 identity, Home Membership, enrollment PoP, mutual device auth, and revocation golden vectors.
- `tests/vectors/aircat/aircat-golden-v1.json`, `aircat-golden-v1.manifest.json`, `aircat-golden-v1.receipt.json` — AirCat Strict v1 manifest SHA-256, signed grant, and receipt vectors.
- `tests/vectors/generated/announce-micro.generated.json` — Canonical signed UDP multicast announce for Michi Micro Server.
- `interop/mobile-readiness/vectors/r1_4_reference_vectors.json` — R1.4 index pointing to the canonical vectors above plus R1.4 Home Roster v1 and strict volume grammar test cases.

## 4. Compatibility & Versioning Rules (v1 Frozen)

- All R1.4 changes are strictly additive or tightening of invalid input rejection (never breaking valid v1 payloads).
- Clients MUST ignore unknown additive JSON fields on response objects (except where a schema explicitly mandates strict validation on client-produced requests).
- Canonical signed byte strings (`michi-home-membership-v1`, `michi-home-enrollment-pop-v1`, `michi-home-auth-v1`, `michi-home-auth-server-v1`, `michi-home-revocation-v1`, `michi-aircat-grant-v1`) are **immutable** across all v1.x releases.
