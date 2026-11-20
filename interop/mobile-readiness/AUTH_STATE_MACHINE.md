# Home Authentication & Membership State Machine (R1.4)

## 1. Identity Primitives (Ed25519 + BLAKE3)

- Scheme: `ed25519-blake3-v1`
- Key encoding: 32-byte raw Ed25519 public key → unpadded Base64URL (43 chars)
- `michi_id = base64urlNoPad(BLAKE3(raw_public_key_32_bytes))` (43 chars)
- Signatures: 64-byte raw Ed25519 signature → unpadded Base64URL (86 chars)
- **Invariant**: `server_id` (UUID) is administrative only and NEVER substitutes for `michi_id`.

## 2. State Machine Transitions

```
DISCOVERED (provisional)
  │  1. Verify GET /api/v1/server/info (identity_scheme + BLAKE3(public_key) == michi_id)
  │  2. Obtain Home trust anchor (scan AirCat Enrollment QR over local UI)
  ▼
TLS_PINNED (pinnedBaseUrl + tlsSpkiSha256 + authorityPublicKeyBase64Url)
  │  3. POST /api/v1/home/enrollment/challenge (HTTPS ONLY)
  │  4. Sign canonical PoP (michi-home-enrollment-pop-v1) with client Ed25519 key
  │  5. POST /api/v1/home/enrollment/request (HTTPS ONLY) → 202 Accepted (request_id, claim_token)
  ▼
ENROLLMENT_PENDING
  │  6. Admin approves via WebUI / POST /api/v1/home/enrollment/{id}/approve (HTTPS ONLY)
  │  7. Poll GET /api/v1/home/enrollment/{id}/status with X-Michi-Enrollment-Claim (HTTPS ONLY)
  │  8. Verify Home Root signature over DeviceMembershipDto (michi-home-membership-v1)
  ▼
ENROLLED_OFFLINE (membership persisted; NOT yet authenticated or online!)
  │  9. POST /api/v1/auth/challenge (HTTPS ONLY)
  │ 10. Sign canonical challenge (michi-home-auth-v1) with client Ed25519 key
  │ 11. POST /api/v1/auth/session (HTTPS ONLY) with membership + client_signature
  │ 12. Verify server_signature over session_token (michi-home-auth-server-v1)
  ▼
AUTHENTICATED_ONLINE (michi_home_* bearer token + X-Michi-Device-Id: <client_michi_id>)
  │  • On token expiry → return to step 9 (re-challenge with persisted membership)
  │  • On 403 MEMBER_REVOKED → immediately wipe session + membership → REVOKED (terminal until re-enrolled)
  │  • On 403 TLS_REQUIRED → fatal client transport bug (never downgrade to HTTP)
  │  • On 503 REVOCATION_STORE_UNAVAILABLE → temporary fail-closed server error (retry with backoff)
```

## 3. Canonical Signed Byte Formats (Immutable v1)

1. **Enrollment Proof-of-Possession** (`michi_identity::sign_home_enrollment_pop`):
   ```
   michi-home-enrollment-pop-v1\n{home_id}\n{server_michi_id}\n{client_michi_id}\n{client_public_key}\n{challenge_id}\n{challenge_nonce}
   ```
2. **Home Membership** (`michi_identity::home::verify_membership`):
   Signed by `home_root_public_key` (`authorityPublicKeyBase64Url`) over canonical membership fields (`version`, `home_id`, `device_michi_id`, `device_public_key`, `device_type`, sorted `roles`, `issued_at`, `expires_at`).
3. **Client Auth Challenge Signature** (`michi_identity::sign_device_auth_challenge`):
   ```
   michi-home-auth-v1\n{home_id}\n{server_michi_id}\n{client_michi_id}\n{challenge_id}\n{challenge_nonce}
   ```
4. **Server Auth Confirmation Signature** (`michi_identity::verify_server_auth_session`):
   Client MUST verify `server_signature` against `server_membership.device_public_key` AND verify `server_membership` against `home_root_public_key` BEFORE storing or using `session_token`.

## 4. Strict Separation from Legacy `SERVER_CODE`

- Legacy `/api/v1/pair/start`, `/api/v1/pair/confirm`, and `/api/v1/pair/qr` (`legacy_server_code_v1`) issue device tokens for legacy LAN players ONLY.
- `SERVER_CODE` tokens are **rejected** on AirCat endpoints (`403 HOME_SESSION_REQUIRED`).
- Legacy QR codes NEVER contain `authorityPublicKeyBase64Url`, `homeId`, or `tlsSpkiSha256`.
