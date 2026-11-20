# AirCat Strict v1 Integration Guide (`michi.aircat.original.v1`)

## 1. Mandatory 5-Flag Eligibility Gate (`AirCatCapabilityPolicy.kt`)

Before showing or enabling AirCat transfers for a server, verify ALL FIVE canonical flags on `GET /api/v1/server/info`:
- `features.aircat_original_v1 == true`
- `features.home_tls_spki_pinned == true`
- `features.aircat_manifest_v1 == true`
- `features.aircat_authorize_v1 == true`
- `features.aircat_original_download_v1 == true`

AND verify active `HOME_MEMBERSHIP` authentication (`michi_home_*` bearer token). Capability flags alone NEVER grant authorization.

## 2. Step-by-Step Original-File Transfer Protocol (HTTPS ONLY)

1. **Fetch Catalog Manifest**:
   - `GET /api/v1/aircat/manifest`
   - Headers: `Authorization: Bearer <michi_home_token>`, `X-Michi-Device-Id: <client_michi_id>`
   - Verify response headers: `X-Michi-AirCat-Protocol: michi.aircat.original.v1`, `Cache-Control: no-store`.
   - Response JSON (`AirCatCatalogManifest`): `{ schema, version, device_id, total_tracks, total_size, tracks: [{ track_id, logical_track_id?, remote_track_id?, title, artist, album, format, size, checksum }] }`.
2. **Build Transfer Manifest & Canonical SHA-256**:
   - Select desired tracks into `Manifest { manifest_version: 1, transfer_id, source_device_id: server_michi_id, total_tracks, total_bytes, items: [...] }`.
   - Compute `manifest_sha256` per `vendor/michi-link/tests/vectors/aircat/aircat-golden-v1.json`.
   - Generate a fresh cryptographic random 24-byte `client_nonce_base64_url` (32 Base64URL chars, single-use).
3. **Request Signed Authorization Grant**:
   - `POST /api/v1/aircat/authorize`
   - Body: `Request { intent, manifest, manifest_sha256, client_nonce_base64_url }` (`aircat-grant-request.schema.json`).
   - Verify `200 OK`, `Grant { version: 1, home_id, destination_michi_id == client_michi_id, manifest_sha256, nonce_base64_url == client_nonce, signature_base64_url (86 chars) }`.
   - Replaying the same nonce returns `409 Conflict`.
4. **Download Original Lossless File (Full or Resumable)**:
   - Encode the raw JSON bytes of `Grant` as unpadded Base64URL → `X-Michi-AirCat-Grant: <base64url(grant_json)>`.
   - `GET /api/v1/aircat/download/{remote_track_id}`
   - Headers: `Authorization: Bearer <michi_home_token>`, `X-Michi-Device-Id: <client_michi_id>`, `X-Michi-AirCat-Grant: <grant_header>`.
   - Verify response headers:
     - `X-Michi-AirCat-Protocol: michi.aircat.original.v1`
     - `Content-Type: application/octet-stream`
     - `Cache-Control: no-store, no-transform`
     - `ETag: "sha256-<hex>-<size>"`
     - `X-Michi-AirCat-Authorized-Grant-SHA256: <hex(SHA256(grant_header_ascii))>`
   - **Resuming interrupted downloads**: Send `Range: bytes=<offset>-` AND `If-Range: <saved_etag>`. Expect `206 Partial Content`. If the file changed on disk, `If-Range` triggers `200 OK` full restart; invalid out-of-bounds range returns `416 Range Not Satisfiable`.
5. **Post-Download Integrity Check**:
   - Compute SHA-256 of the reassembled file on disk; verify exact equality with `checksum` from manifest before committing to local library.
