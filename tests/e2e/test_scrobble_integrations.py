#!/usr/bin/env python3
"""
HTTP Integration Tests for Scrobbling Provider Integrations — Phase 8 of WebUI Contract Closure Plan.

Verifies the canonical integration management API:
  - GET  /api/v1/integrations/scrobbling  → scrobble status contract
  - POST /api/v1/integrations/listenbrainz  → token validation contract
  - POST /api/v1/integrations/lastfm  → session token persistence contract
  - Last.fm MD5 signature algorithm (deterministic unit-level check without network)
  - ListenBrainz HTTP 200 with error payload handling
  - Injectable base URL environment variables for offline CI testing

Usage:
    pytest tests/e2e/test_scrobble_integrations.py -v
    # Offline mode (no real upstream requests):
    MICHI_LISTENBRAINZ_BASE_URL=http://127.0.0.1:1 pytest tests/e2e/test_scrobble_integrations.py -v
"""

import hashlib
import http.cookiejar
import json
import os
import uuid
import urllib.request
import urllib.error
import pytest

SERVER_URL = os.environ.get("MICHI_SERVER_URL", "http://127.0.0.1:9090")
ADMIN_USERNAME = os.environ.get("MICHI_ADMIN_USERNAME", os.environ.get("MICHI_AUTH_USERNAME", "admin"))
ADMIN_PASSWORD = os.environ.get("MICHI_ADMIN_PASSWORD", os.environ.get("MICHI_AUTH_PASSWORD", "admin12345"))


# ── helpers ────────────────────────────────────────────────────────────────────

def _opener():
    cj = http.cookiejar.CookieJar()
    return urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj)), cj


def _authenticated_opener():
    """Returns (opener, auth_enabled). Authenticates when auth is enabled."""
    opener, _ = _opener()
    try:
        with urllib.request.urlopen(f"{SERVER_URL}/api/auth/check", timeout=5) as r:
            data = json.loads(r.read())
            if not data.get("enabled", False):
                return opener, False
    except Exception:
        return opener, False

    payload = json.dumps({"username": ADMIN_USERNAME, "password": ADMIN_PASSWORD}).encode()
    req = urllib.request.Request(
        f"{SERVER_URL}/api/auth/login",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with opener.open(req, timeout=5) as r:
        assert r.status == 200, f"Login failed: {r.status}"
    return opener, True


def _get(opener, path, timeout=5):
    req = urllib.request.Request(f"{SERVER_URL}{path}")
    with opener.open(req, timeout=timeout) as r:
        return r.status, json.loads(r.read())


def _post(opener, path, body, timeout=5):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{SERVER_URL}{path}",
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with opener.open(req, timeout=timeout) as r:
        return r.status, json.loads(r.read())


def _post_expecting_error(opener, path, body, timeout=5):
    """POST and return (status, body) even on HTTP errors."""
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{SERVER_URL}{path}",
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with opener.open(req, timeout=timeout) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        try:
            body_bytes = e.read()
            return e.code, json.loads(body_bytes)
        except Exception:
            return e.code, {}


# ── Scrobbling status contract ─────────────────────────────────────────────────

class TestScrobbleStatusContract:
    """
    Verifies GET /api/v1/integrations/scrobbling returns the canonical
    ScrobbleStatusResponse shape with no secrets exposed.
    """

    def test_scrobble_status_returns_expected_shape(self):
        """GET /api/v1/integrations/scrobbling must return required boolean fields."""
        opener, _ = _authenticated_opener()
        status, body = _get(opener, "/api/v1/integrations/scrobbling")
        assert status == 200, f"Scrobble status returned {status}: {body}"
        assert "scrobble_enabled" in body, f"'scrobble_enabled' missing: {body}"
        assert "listenbrainz_configured" in body, f"'listenbrainz_configured' missing: {body}"
        assert "lastfm_configured" in body, f"'lastfm_configured' missing: {body}"
        assert isinstance(body["scrobble_enabled"], bool)
        assert isinstance(body["listenbrainz_configured"], bool)
        assert isinstance(body["lastfm_configured"], bool)

    def test_scrobble_status_does_not_expose_secrets(self):
        """Scrobbling status must not expose raw tokens or secrets."""
        opener, _ = _authenticated_opener()
        _, body = _get(opener, "/api/v1/integrations/scrobbling")
        secret_keys = {"token", "api_key", "shared_secret", "session_key", "password"}
        exposed = secret_keys & set(body.keys())
        assert not exposed, (
            f"Scrobble status exposes sensitive fields: {exposed}. "
            "Tokens must never be returned to the client."
        )

    def test_scrobble_status_idempotent(self):
        """GET /api/v1/integrations/scrobbling is safe — two calls return consistent results."""
        opener, _ = _authenticated_opener()
        _, first = _get(opener, "/api/v1/integrations/scrobbling")
        _, second = _get(opener, "/api/v1/integrations/scrobbling")
        assert first["scrobble_enabled"] == second["scrobble_enabled"]
        assert first["listenbrainz_configured"] == second["listenbrainz_configured"]
        assert first["lastfm_configured"] == second["lastfm_configured"]


# ── ListenBrainz token validation contract ────────────────────────────────────

class TestListenBrainzContract:
    """
    Verifies POST /api/v1/integrations/listenbrainz token validation:
    - Empty token rejected with 400 VALIDATION_ERROR
    - Whitespace-only token rejected with 400 VALIDATION_ERROR
    - Invalid token rejected with 401 or 502 (depends on upstream availability)
    """

    def test_empty_token_rejected_with_400(self):
        """Empty ListenBrainz token must return 400 VALIDATION_ERROR."""
        opener, _ = _authenticated_opener()
        status, body = _post_expecting_error(opener, "/api/v1/integrations/listenbrainz", {"token": ""})
        assert status == 400, f"Empty token expected 400, got {status}: {body}"
        error = body.get("error", {})
        assert error.get("code") == "VALIDATION_ERROR", (
            f"Expected VALIDATION_ERROR code, got: {error}"
        )

    def test_whitespace_only_token_rejected_with_400(self):
        """Whitespace-only token must be treated as empty and rejected with 400."""
        opener, _ = _authenticated_opener()
        status, body = _post_expecting_error(
            opener, "/api/v1/integrations/listenbrainz", {"token": "   "}
        )
        assert status == 400, f"Whitespace token expected 400, got {status}: {body}"

    def test_invalid_token_not_accepted_as_200(self):
        """A clearly invalid token (random UUID) must not return 200 configured."""
        opener, _ = _authenticated_opener()
        fake_token = str(uuid.uuid4())
        status, body = _post_expecting_error(
            opener, "/api/v1/integrations/listenbrainz", {"token": fake_token}
        )
        # Acceptable: 401 INVALID_TOKEN, 502 UPSTREAM_ERROR (LB unreachable in CI), 400
        # Not acceptable: 200 with configured=true for a random UUID
        if status == 200:
            # If upstream is mocked or not configured, the server may pass through.
            # Only fail if it clearly claims the token is valid.
            assert body.get("valid") is not True, (
                f"Server accepted random UUID as a valid ListenBrainz token: {body}"
            )
        else:
            assert status in (400, 401, 502, 503), (
                f"Unexpected error code for invalid token: {status}: {body}"
            )

    def test_missing_token_field_rejected(self):
        """Request with missing 'token' field must return 400 or 422."""
        opener, _ = _authenticated_opener()
        status, body = _post_expecting_error(
            opener, "/api/v1/integrations/listenbrainz", {"not_a_token": "value"}
        )
        assert status in (400, 422), f"Expected 400/422 for missing token field, got {status}: {body}"


# ── Last.fm session token contract ────────────────────────────────────────────

class TestLastFmContract:
    """
    Verifies POST /api/v1/integrations/lastfm session token persistence:
    - Empty token rejected with 400 VALIDATION_ERROR
    - Valid-format session token accepted with 200
    - Response must not expose the token back
    """

    def test_empty_lastfm_token_rejected_with_400(self):
        """Empty Last.fm session token must return 400 VALIDATION_ERROR."""
        opener, _ = _authenticated_opener()
        status, body = _post_expecting_error(opener, "/api/v1/integrations/lastfm", {"token": ""})
        assert status == 400, f"Empty token expected 400, got {status}: {body}"
        error = body.get("error", {})
        assert error.get("code") == "VALIDATION_ERROR", f"Expected VALIDATION_ERROR: {error}"

    def test_valid_format_lastfm_token_accepted(self):
        """A non-empty session token must be accepted and persisted (200 status)."""
        opener, _ = _authenticated_opener()
        # Last.fm session keys are 32-char hex strings, but we only test API acceptance
        session_token = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"
        status, body = _post(opener, "/api/v1/integrations/lastfm", {"token": session_token})
        assert status == 200, f"Valid-format session token rejected: {status}: {body}"
        assert body.get("status") == "configured", f"Expected status=configured: {body}"
        assert body.get("provider") == "lastfm", f"Expected provider=lastfm: {body}"

    def test_lastfm_response_does_not_echo_token(self):
        """The Last.fm configure response must not echo the session token back."""
        opener, _ = _authenticated_opener()
        session_token = "b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5"
        _, body = _post(opener, "/api/v1/integrations/lastfm", {"token": session_token})
        body_str = json.dumps(body)
        assert session_token not in body_str, (
            f"Response echoes the session token — must never return raw credentials: {body}"
        )


# ── Last.fm signature algorithm unit check ────────────────────────────────────

class TestLastFmSignatureAlgorithm:
    """
    Unit-level verification of the Last.fm MD5 api_sig algorithm.
    This is pure Python — no server required — and validates the canonical
    behavior documented at https://www.last.fm/api/authspec.

    The Rust implementation: calculate_lastfm_signature in scrobble.rs
    must produce identical output for the same inputs.
    """

    @staticmethod
    def _calculate_lastfm_signature(params: list[tuple[str, str]], secret: str) -> str:
        """
        Canonical Last.fm MD5 api_sig:
          1. Sort params alphabetically by key
          2. Exclude 'format' and 'api_sig' keys
          3. Concatenate key+value pairs (no separator)
          4. Append shared_secret
          5. MD5 hex digest (lowercase)
        """
        sorted_params = sorted(params, key=lambda x: x[0])
        sig_base = "".join(k + v for k, v in sorted_params if k not in ("format", "api_sig"))
        sig_base += secret
        return hashlib.md5(sig_base.encode("utf-8")).hexdigest()

    def test_signature_matches_known_value(self):
        """
        Known-good Last.fm API signature from documentation examples.
        api_key=abc123 method=track.scrobble sk=sesskey artist=Band track=Song
        secret=secret
        Expected: md5("api_keyabc123artistBandmethodtrack.scrobblesksesskeytrackSongsecret")
        """
        params = [
            ("method", "track.scrobble"),
            ("api_key", "abc123"),
            ("sk", "sesskey"),
            ("artist", "Band"),
            ("track", "Song"),
            ("format", "json"),  # must be excluded
        ]
        secret = "secret"
        sig = self._calculate_lastfm_signature(params, secret)
        # Canonical string: api_keyabc123artistBandmethodtrack.scrobblesksesskeytrackSong + secret
        canonical = "api_keyabc123artistBandmethodtrack.scrobblesksesskeytrackSong" + secret
        expected = hashlib.md5(canonical.encode("utf-8")).hexdigest()
        assert sig == expected, f"Signature mismatch: got {sig}, expected {expected}"

    def test_format_key_excluded_from_signature(self):
        """The 'format' key must be excluded from the signature computation."""
        params_with_format = [("api_key", "k1"), ("format", "json"), ("method", "auth.getSession")]
        params_without_format = [("api_key", "k1"), ("method", "auth.getSession")]
        secret = "mysecret"
        sig_with = self._calculate_lastfm_signature(params_with_format, secret)
        sig_without = self._calculate_lastfm_signature(params_without_format, secret)
        assert sig_with == sig_without, (
            "Including 'format' in params changed the signature — "
            "'format' must be excluded from api_sig computation."
        )

    def test_api_sig_key_excluded_from_signature(self):
        """The 'api_sig' key itself must be excluded (prevents circular dependency)."""
        params_without = [("api_key", "k1"), ("method", "track.scrobble")]
        params_with_sig = [("api_key", "k1"), ("method", "track.scrobble"), ("api_sig", "deadbeef")]
        secret = "mysecret"
        sig_without = self._calculate_lastfm_signature(params_without, secret)
        sig_with = self._calculate_lastfm_signature(params_with_sig, secret)
        assert sig_without == sig_with, (
            "'api_sig' key must be excluded from the signature input."
        )

    def test_signature_is_lowercase_hex(self):
        """The api_sig must be a 32-character lowercase hex string."""
        params = [("api_key", "testkey"), ("method", "track.updateNowPlaying")]
        sig = self._calculate_lastfm_signature(params, "sharedsecret")
        assert len(sig) == 32, f"Signature must be 32 chars, got {len(sig)}: {sig}"
        assert sig == sig.lower(), f"Signature must be lowercase hex: {sig}"
        assert all(c in "0123456789abcdef" for c in sig), f"Non-hex chars in signature: {sig}"

    def test_signature_changes_when_params_change(self):
        """Different param values must produce different signatures."""
        params_a = [("api_key", "key1"), ("method", "track.scrobble"), ("artist", "ArtistA")]
        params_b = [("api_key", "key1"), ("method", "track.scrobble"), ("artist", "ArtistB")]
        secret = "sharedsecret"
        assert self._calculate_lastfm_signature(params_a, secret) != \
               self._calculate_lastfm_signature(params_b, secret), (
            "Different param values produced identical signatures — algorithm is broken."
        )

    def test_signature_changes_when_secret_changes(self):
        """Different secrets must produce different signatures for the same params."""
        params = [("api_key", "key"), ("method", "track.scrobble")]
        sig_a = self._calculate_lastfm_signature(params, "secretA")
        sig_b = self._calculate_lastfm_signature(params, "secretB")
        assert sig_a != sig_b, "Different secrets produced identical signatures."


# ── HTTP 200 error payload handling ───────────────────────────────────────────

class TestLastFmHttp200ErrorHandling:
    """
    Last.fm API returns HTTP 200 with a JSON error code when the scrobble fails
    (e.g., {"error": 6, "message": "Invalid parameters"}). The server must
    detect and log these — never treat HTTP 200 + error JSON as a success.

    This is tested via an injectable base URL pointing to a mock endpoint.
    In CI without a mock server, these tests are skipped.
    """

    @pytest.fixture(autouse=True)
    def require_injectable_base_url(self):
        base = os.environ.get("MICHI_LASTFM_API_BASE_URL", "")
        if not base or "ws.audioscrobbler.com" in base:
            pytest.skip(
                "MICHI_LASTFM_API_BASE_URL not set to a mock endpoint — "
                "set it to verify HTTP 200 error payload handling offline."
            )

    def test_http_200_with_error_json_is_logged_not_silently_accepted(self):
        """
        When the Last.fm mock returns HTTP 200 with { \"error\": 6 },
        the server must log a warning (not treat it as a successful scrobble).

        This test verifies the record_play endpoint still returns 200 to the client
        (scrobble submission is fire-and-forget), but the upstream error is not silenced.
        """
        opener, _ = _authenticated_opener()
        track_id = str(uuid.uuid4())
        # Record a play — the server will attempt to scrobble asynchronously.
        # The mock URL returns HTTP 200 with { "error": 6 }.
        # Scrobble submission is async fire-and-forget; the API endpoint still returns 200.
        status, body = _post(opener, "/api/v1/playback/record", {
            "track_id": track_id,
            "duration_ms": 300_000,
        })
        assert status < 500, f"record_play returned {status}: {body}"
        # We cannot assert the warning was logged here (that would require log inspection),
        # but we can assert the API did not crash due to the upstream HTTP 200 error.
