# TLS & X.509 Trust Policy (R1.4)

## 1. Mandatory HTTPS Enforcement

All Home Authority, Enrollment, Mutual Auth, AirCat, and `michi_home_*` session calls MUST go over HTTPS (`pinnedBaseUrl`):
- Cleartext HTTP receives `403 TLS_REQUIRED` (when TLS listener is active) or `503 TLS_NOT_CONFIGURED` (when TLS is not configured).
- Spoofed `X-Forwarded-Proto: https` or `Forwarded: proto=https` from untrusted peers are unconditionally ignored.
- Contradictory forwarding headers from a trusted reverse proxy (e.g. `X-Forwarded-Proto: https` + `Forwarded: proto=http`) fail closed with `403 TLS_REQUIRED`.

## 2. Dual-Layer Verification Required in Android (OkHttp)

Android clients MUST enforce **BOTH**:
1. **Standard X.509 Chain & Hostname Verification** — default platform `X509TrustManager` (plus optional explicit user/app Network Security Config CA for private LAN CAs) AND default `HostnameVerifier`.
   - **FORBIDDEN**: Trust-all `X509TrustManager`, empty `checkServerTrusted`, or `HostnameVerifier { _, _ -> true }`.
2. **SPKI SHA-256 Pinning** — OkHttp `CertificatePinner` pinned to `tlsSpkiSha256` from the scanned `AirCatTrustedEnrollmentQr`.
   - Exact wire format: `sha256/<44-char-standard-base64>` (total length **51 chars**, matching OkHttp `CertificatePinner` natively!).

## 3. Server TLS Status Honesty Semantics (`GET /api/v1/aircat/status`)

Micro Server strictly separates **server-side X.509 chain verification** from **Android device trust**:

| Field | Values | Meaning |
| --- | --- | --- |
| `tls_trust_mode` | `ca_chained` \| `self_signed` | Whether leaf is chained to an issuing CA or self-signed |
| `certificate_profile` | `ca_issued_chain_present` \| `self_signed` | Structural X.509 profile inspected on disk |
| `chain_cryptographically_verified` | `true` \| `false` | Server verified leaf → issuer signature cryptographically |
| `android_trust_verification_status` | `not_verified` | **Honest truth**: server NEVER claims Android trusts a private CA unless a real Android device verifies it! |

## 4. Production Deployment Recommendations (ZimaOS / Docker)

- **Recommended (Zero custom Android CA)**: Reverse proxy (Caddy/Traefik/Nginx) with Let's Encrypt DNS-01 certificate OR Tailscale HTTPS (`*.ts.net`), plus `MICHI_TRUST_PROXY=true`, `MICHI_TRUSTED_PROXIES=<proxy_ip>`, and `MICHI_PUBLIC_HTTPS_URL=https://<domain>`.
- **Direct Native TLS (`MICHI_TLS_CERT_PATH` / `MICHI_TLS_KEY_PATH`)**: Supply a CA-chained fullchain PEM with SAN matching the LAN IP/hostname (`DNS:<host>,IP:<lan_ip>`) and install the issuing private CA into the Android test device (or debug Network Security Config for dev builds).
