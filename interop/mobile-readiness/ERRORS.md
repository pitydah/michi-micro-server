# Error Handling Contract (R1.4)

All v1 error responses follow `vendor/michi-link/schemas/error.schema.json`:

```json
{
  "error": {
    "code": "ERROR_CODE",
    "message": "Human-readable diagnostic",
    "details": {}
  }
}
```

## Mandatory Client Handling Matrix

| HTTP | Error Code | Meaning | Required Mobile Action |
| --- | --- | --- | --- |
| `400` | `INVALID_REQUEST` / `COUNT_MISMATCH` / `DUPLICATE_ITEM_ID` / `UNKNOWN_ITEM_ID` | Invalid payload (e.g. leading-zero volume `"075"`, invalid QR host, stale reorder IDs) | Do not retry verbatim; refresh queue/form state and report error |
| `401` | `UNAUTHORIZED` / `SESSION_EXPIRED` | Missing, malformed, or expired bearer token | Trigger mutual re-authentication (`POST /api/v1/auth/challenge` → `/auth/session`) using persisted `membership` |
| `403` | `TLS_REQUIRED` | Sensitive Home/AirCat route or `michi_home_*` token sent over cleartext HTTP or contradictory proxy headers | **FATAL transport guard**: abort cleartext, switch strictly to verified HTTPS `pinnedBaseUrl` |
| `403` | `MEMBER_REVOKED` / `DEVICE_REVOKED` | Device was revoked by Home Root Authority | **Immediately wipe** cached `michi_home_*` token AND persisted `membership`; show revoked UX requiring fresh enrollment |
| `403` | `HOME_SESSION_REQUIRED` | Legacy `SERVER_CODE` token sent to Home/AirCat endpoint | Prompt user to complete Home Enrollment (do not retry with legacy token) |
| `403` | `IDENTITY_MISMATCH` / `HOME_ID_MISMATCH` | Key/Michi ID or Home ID mismatch | Abort connection and alert user to possible server identity change |
| `409` | `NONCE_REPLAY` / `CONFLICT` | AirCat authorize nonce already used or pairing already consumed | Generate a fresh random 24-byte `client_nonce_base64_url` and retry once |
| `410` | `GONE` | Legacy QR pairing code expired | Request a new QR code from server UI |
| `415` | `UNSUPPORTED_MEDIA_TYPE` | Missing `Content-Type: application/json` on `POST`/`PUT` | Always set `Content-Type: application/json` on every mutating request (even with `{}` body) |
| `416` | `RANGE_NOT_SATISFIABLE` | AirCat `Range` offset exceeds file size | Restart file download from byte `0` |
| `429` | `RATE_LIMITED` / Too Many Requests | Per-IP rate limit exceeded (default `10 rps / burst 20`, pairing `5/min`, login `10/min`) | Respect `Retry-After` / exponential backoff (never tight-loop retries) |
| `503` | `TLS_NOT_CONFIGURED` | Server has no valid HTTPS listener configured for Home/AirCat | Show server setup guidance (configure TLS or trusted HTTPS reverse proxy) |
| `503` | `REVOCATION_STORE_UNAVAILABLE` | Server cannot verify revocation status fail-closed | Treat as temporary outage; retry with backoff; never assume device is authorized |
| `503` | `QUEUE_DEGRADED` | Server queue storage & PlaybackEngine diverged after rollback failure | Display degraded queue banner and offer explicit reconcile (`POST /api/v1/queue/reconcile`) |
| `503` | `MODULE_DISABLED` | Requested module (e.g. `playback`) disabled on server | Refresh `/api/v1/capabilities` & `/api/v1/server/info` and disable corresponding UI action |
