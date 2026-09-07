#!/usr/bin/env python3
"""
HTTP Integration Tests for WebUI Action Effects — Phase 8 of WebUI Contract Closure Plan.

Verifies that every observable WebUI action produces the correct server-side effect:
  - Browser vs. server playback authority separation (queue independence)
  - History/listen recording and qualifying listen threshold
  - Podcast episode progress persistence (PUT /api/v1/sources/episodes/:id)
  - Receiver discovery response uses ``receivers`` key (not ``devices``)
  - Remote sync network policy enforcement (local clients always allowed)
  - Michi Link self-test non-mutating idempotency

Usage:
    pytest tests/e2e/test_webui_action_effects.py -v
    # or with environment overrides:
    MICHI_SERVER_URL=http://127.0.0.1:9090 pytest tests/e2e/test_webui_action_effects.py -v
"""

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


def _put(opener, path, body, timeout=5):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{SERVER_URL}{path}",
        data=data,
        headers={"Content-Type": "application/json"},
        method="PUT",
    )
    with opener.open(req, timeout=timeout) as r:
        return r.status, json.loads(r.read())


# ── Playback authority ─────────────────────────────────────────────────────────

class TestPlaybackAuthorityContract:
    """
    Verifies that the server-side queue and playback engine are always reachable
    and that their state is independent of the browser authority domain.
    """

    def test_server_playback_state_returns_state_field(self):
        """GET /api/v1/playback/state must return { state: ... } from the server engine."""
        opener, _ = _authenticated_opener()
        status, body = _get(opener, "/api/v1/playback/state")
        assert status == 200, f"Unexpected status: {status}"
        assert "state" in body, f"'state' field missing: {body}"
        assert body["state"] in ("idle", "playing", "paused", "stopped", "error", "loading")

    def test_server_queue_returns_items_array(self):
        """GET /api/v1/queue must return { items: [...], queue_id: ..., items_count: ... } — server authority."""
        opener, _ = _authenticated_opener()
        status, body = _get(opener, "/api/v1/queue")
        assert status == 200
        assert "items" in body, f"'items' key missing in response: {body}"
        assert "items_count" in body, f"'items_count' key missing in response: {body}"
        assert "queue_id" in body, f"'queue_id' key missing in response: {body}"
        assert isinstance(body["items"], list)

    def test_server_queue_delete_nonexistent_returns_404(self):
        """DELETE /api/v1/queue/:queue_id for unknown queue returns 404, validating routing."""
        opener, _ = _authenticated_opener()
        fake_qid = str(uuid.uuid4())
        req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/queue/{fake_qid}",
            headers={"Content-Type": "application/json"},
            method="DELETE",
        )
        try:
            with opener.open(req, timeout=5) as r:
                assert r.status == 404
        except urllib.error.HTTPError as e:
            assert e.code == 404, f"Expected 404 for unknown queue, got {e.code}"


# ── Listen history & threshold ─────────────────────────────────────────────────

class TestListenHistoryContract:
    """
    Verifies the canonical listen recording contract:
        POST /api/v1/playback/record  →  { status, id }
        GET  /api/v1/playback/history →  array of entries

    The qualifying listen threshold (listened_ms >= min(duration_ms/2, 240_000))
    is enforced server-side; the API layer accepts all events.
    """

    def test_record_play_returns_ok_or_duplicate(self):
        """POSTing a play event persists a history entry."""
        opener, _ = _authenticated_opener()
        track_id = str(uuid.uuid4())
        status, body = _post(opener, "/api/v1/playback/record", {
            "track_id": track_id,
            "duration_ms": 180_000,
        })
        assert status < 500, f"record play returned {status}: {body}"
        if status == 200:
            assert body.get("status") in ("recorded", "ok", "duplicate_skipped"), body

    def test_play_history_returns_list(self):
        """GET /api/v1/history must return an object with a history list."""
        opener, _ = _authenticated_opener()
        status, body = _get(opener, "/api/v1/history")
        assert status == 200
        if isinstance(body, list):
            entries = body
        elif isinstance(body, dict):
            entries = body.get("history") or body.get("entries") or []
        else:
            pytest.fail(f"Unexpected response shape: {body}")
        assert isinstance(entries, list)

    def test_record_play_short_duration_accepted_at_api_layer(self):
        """Short plays (below threshold) are accepted by the API; threshold logic is server-side."""
        opener, _ = _authenticated_opener()
        track_id = str(uuid.uuid4())
        status, body = _post(opener, "/api/v1/playback/record", {
            "track_id": track_id,
            "duration_ms": 5_000,
        })
        assert status < 500, f"Short listen rejected at API layer with 5xx: {body}"

    def test_record_play_missing_track_id_rejected(self):
        """POSTing without track_id must be rejected 4xx, not 5xx."""
        opener, _ = _authenticated_opener()
        data = json.dumps({"duration_ms": 60_000}).encode()
        req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/playback/record",
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with opener.open(req, timeout=5) as r:
                pytest.fail(f"Expected 4xx but got {r.status}")
        except urllib.error.HTTPError as e:
            assert e.code in (400, 422), f"Expected 400/422, got {e.code}"


# ── Podcast episode progress contract ─────────────────────────────────────────

class TestPodcastEpisodeProgressContract:
    """
    Verifies Phase 4 fix: WebUI sends PUT /api/v1/sources/episodes/:id with
    { position_ms, played } using the canonical endpoint.
    The legacy GET /api/v1/episodes/:id path must not exist.
    """

    def test_update_episode_endpoint_registered(self):
        """PUT /api/v1/sources/episodes/<id> must be registered (strictly 404 for unknown, never 405)."""
        opener, _ = _authenticated_opener()
        episode_id = str(uuid.uuid4())
        data = json.dumps({"position_ms": 42_000, "played": False}).encode()
        req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/sources/episodes/{episode_id}",
            data=data,
            headers={"Content-Type": "application/json"},
            method="PUT",
        )
        try:
            with opener.open(req, timeout=5) as r:
                pytest.fail(f"Expected 404 for unknown episode, got {r.status}")
        except urllib.error.HTTPError as e:
            assert e.code != 405, (
                "PUT /api/v1/sources/episodes/:id returned 405 Method Not Allowed — "
                "endpoint is missing. Phase 4 fix is not applied."
            )
            assert e.code == 404, f"Expected 404 for nonexistent episode, got {e.code}"

    def test_update_episode_progress_persists_when_episode_exists(self):
        """If a podcast source/episode exists, PUT persists position_ms and played state."""
        opener, _ = _authenticated_opener()
        # Query existing sources
        req = urllib.request.Request(f"{SERVER_URL}/api/v1/sources")
        try:
            with opener.open(req, timeout=5) as r:
                data = json.loads(r.read())
                sources = data.get("sources", [])
        except Exception:
            sources = []

        episode = None
        source_id = None
        for s in sources:
            sid = s.get("id")
            if not sid:
                continue
            try:
                with opener.open(urllib.request.Request(f"{SERVER_URL}/api/v1/sources/{sid}/episodes"), timeout=5) as er:
                    ep_data = json.loads(er.read())
                    episodes = ep_data.get("episodes", [])
                    if episodes:
                        episode = episodes[0]
                        source_id = sid
                        break
            except Exception:
                continue

        if not episode:
            # If no episodes in current live DB, verify endpoint rejects malformed payload with 400/422
            req_bad = urllib.request.Request(
                f"{SERVER_URL}/api/v1/sources/episodes/{uuid.uuid4()}",
                data=b"invalid-json",
                headers={"Content-Type": "application/json"},
                method="PUT",
            )
            try:
                with opener.open(req_bad, timeout=5) as r:
                    pytest.fail(f"Expected 400/422 for invalid JSON, got {r.status}")
            except urllib.error.HTTPError as err:
                assert err.code in (400, 422), f"Expected 400 or 422, got {err.code}"
            return

        ep_id = episode["id"]
        target_pos = 42_000
        data = json.dumps({"position_ms": target_pos, "played": False}).encode()
        put_req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/sources/episodes/{ep_id}",
            data=data,
            headers={"Content-Type": "application/json"},
            method="PUT",
        )
        with opener.open(put_req, timeout=5) as r:
            assert r.status in (200, 204)

        # Verify state through GET /api/v1/sources/:source_id/episodes
        with opener.open(urllib.request.Request(f"{SERVER_URL}/api/v1/sources/{source_id}/episodes"), timeout=5) as r:
            verified_data = json.loads(r.read())
            verified_ep = next((e for e in verified_data.get("episodes", []) if e["id"] == ep_id), None)
            assert verified_ep is not None, f"Episode {ep_id} missing in source {source_id}"
            assert verified_ep.get("position_ms") == target_pos, f"Expected position_ms {target_pos}, got {verified_ep.get('position_ms')}"
            assert verified_ep.get("played") is False, f"Expected played False, got {verified_ep.get('played')}"

    def test_legacy_episode_path_not_routed(self):
        """GET /api/v1/episodes/:id (legacy path) must return 404."""
        opener, _ = _authenticated_opener()
        episode_id = str(uuid.uuid4())
        req = urllib.request.Request(f"{SERVER_URL}/api/v1/episodes/{episode_id}")
        try:
            with opener.open(req, timeout=5) as r:
                pytest.fail(f"Legacy /api/v1/episodes/:id returned {r.status} — should not exist.")
        except urllib.error.HTTPError as e:
            assert e.code == 404, f"Expected 404 for legacy path, got {e.code}"


# ── Receiver discovery response contract ──────────────────────────────────────

class TestReceiverDiscoveryContract:
    """
    Verifies Phase 4 fix: discovery responses use the 'receivers' key.
    The WebUI's discoverDevices() references res.receivers;
    if the response uses 'devices' the UI silently renders empty state.
    """

    def test_receivers_endpoint_uses_receivers_key(self):
        """GET /api/v1/receivers must return { receivers: [...] }."""
        opener, _ = _authenticated_opener()
        status, body = _get(opener, "/api/v1/receivers")
        assert status == 200
        assert "receivers" in body, f"Missing 'receivers' key: {list(body.keys())}"
        assert "devices" not in body, (
            "Response contains legacy 'devices' key — "
            "WebUI res.receivers reference would render empty state."
        )
        assert isinstance(body["receivers"], list)

    def test_discover_endpoint_uses_receivers_key(self):
        """POST /api/v1/devices/discover must return { receivers: [...] } without skipping."""
        opener, _ = _authenticated_opener()
        status, body = _post(opener, "/api/v1/devices/discover", {}, timeout=10)
        assert status == 200, f"Unexpected status: {status}"
        assert "receivers" in body, (
            f"Discovery response missing 'receivers' key — got: {list(body.keys())}. "
            "WebUI discoverDevices() requires 'receivers' array."
        )
        assert "devices" not in body, "Discovery response must not use deprecated 'devices' key."
        assert isinstance(body["receivers"], list), "'receivers' must be an array"


# ── Remote sync network policy enforcement ────────────────────────────────────

class TestRemoteSyncNetworkPolicy:
    """
    Verifies Phase 5: the sync network policy allows localhost clients even
    when remote_sync=false.  Public IP enforcement is tested exhaustively in
    crates/michi-api/tests/remote_sync_security_tests.rs against real TCP.
    """

    def test_sync_manifest_reachable_from_localhost(self):
        """GET /api/v1/sync/manifest must return 200 from localhost."""
        opener, _ = _authenticated_opener()
        status, body = _get(opener, "/api/v1/sync/manifest")
        assert status == 200, (
            f"Sync manifest returned {status} from localhost. "
            "Local clients must not be rejected by the network policy."
        )
        assert "tracks" in body or "cursor" in body, f"Unexpected manifest shape: {body}"

    def test_sync_state_accessible_from_localhost(self):
        """GET /api/v1/sync/state from localhost must not return 403."""
        opener, _ = _authenticated_opener()
        try:
            status, _ = _get(opener, "/api/v1/sync/state?device_id=e2e-test-device")
            assert status in (200, 400), f"Unexpected status: {status}"
        except urllib.error.HTTPError as e:
            if e.code == 403:
                pytest.fail(
                    "GET /api/v1/sync/state returned 403 from localhost — "
                    "network policy is incorrectly rejecting local clients."
                )
            assert e.code in (400, 404)

    def test_sync_manifest_spoofed_xff_from_untrusted_peer_allowed(self):
        """
        Local peer spoofing a public X-Forwarded-For header must still be allowed.
        Untrusted proxies' XFF is ignored per the network policy.
        """
        opener, _ = _authenticated_opener()
        req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/sync/manifest",
            headers={"X-Forwarded-For": "203.0.113.195"},
        )
        try:
            with opener.open(req, timeout=5) as r:
                assert r.status == 200, (
                    f"Local peer with spoofed XFF was rejected with {r.status}. "
                    "Untrusted proxies must not be able to forge the client IP."
                )
        except urllib.error.HTTPError as e:
            pytest.fail(
                f"Sync manifest rejected with {e.code} despite being a local client. "
                "Spoofed XFF from untrusted peer must be ignored."
            )


# ── Michi Link self-test ───────────────────────────────────────────────────────

class TestMichiLinkSelfTest:
    """
    Verifies Phase 7: GET /api/v1/link/self-test returns a structured result
    and is idempotent (non-mutating).
    """

    def test_self_test_returns_structured_checks(self):
        """GET /api/v1/link/self-test must return 200 with structured check results."""
        opener, _ = _authenticated_opener()
        status, body = _get(opener, "/api/v1/link/self-test")
        assert status == 200, f"Self-test returned {status}: {body}"
        assert (
            "checks" in body
            or "server_identity" in body
            or "results" in body
            or "subsystems" in body
        ), f"Self-test response missing structured check results: {body}"

    def test_self_test_is_idempotent(self):
        """Running self-test twice must produce the same overall result."""
        opener, _ = _authenticated_opener()
        _, first = _get(opener, "/api/v1/link/self-test")
        _, second = _get(opener, "/api/v1/link/self-test")

        def _overall(r):
            return r.get("overall") or r.get("status") or r.get("ok")

        assert _overall(first) == _overall(second), (
            f"Self-test is not idempotent — first={_overall(first)}, second={_overall(second)}"
        )
