#!/usr/bin/env python3
"""
HTTP Functional Integration Test for Michi WebUI.
Verifies all WebUI control contracts, state truthfulness, authentication lifecycle,
and API contracts.

Usage:
    pytest tests/e2e/test_webui_functional_conformance.py
    # or with custom server and credentials:
    MICHI_SERVER_URL=http://127.0.0.1:9090 MICHI_ADMIN_USERNAME=admin MICHI_ADMIN_PASSWORD=admin12345 pytest tests/e2e/test_webui_functional_conformance.py
"""

import http.cookiejar
import json
import os
import urllib.request
import urllib.error
import pytest

SERVER_URL = os.environ.get("MICHI_SERVER_URL", "http://127.0.0.1:9090")
ADMIN_USERNAME = os.environ.get("MICHI_ADMIN_USERNAME", os.environ.get("MICHI_AUTH_USERNAME", "admin"))
ADMIN_PASSWORD = os.environ.get("MICHI_ADMIN_PASSWORD", os.environ.get("MICHI_AUTH_PASSWORD", "admin12345"))


def _get_opener():
    cj = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
    return opener, cj


def _get_authenticated_opener():
    opener, cj = _get_opener()
    # Check if auth is enabled
    try:
        req = urllib.request.Request(f"{SERVER_URL}/api/auth/check")
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            if not data.get("enabled", False):
                # Auth disabled on this test instance
                return opener, cj, False
    except Exception:
        pass

    # Perform real login
    login_payload = json.dumps({
        "username": ADMIN_USERNAME,
        "password": ADMIN_PASSWORD
    }).encode("utf-8")

    req = urllib.request.Request(
        f"{SERVER_URL}/api/auth/login",
        data=login_payload,
        headers={"Content-Type": "application/json"},
        method="POST"
    )

    with opener.open(req, timeout=5) as resp:
        assert resp.status == 200, f"Login failed with status {resp.status}"
        data = json.loads(resp.read().decode("utf-8"))
        assert "token" in data or data.get("status") == "ok" or "user" in data

    # Verify cookie was set
    cookie_names = [c.name for c in cj]
    assert "michi_web_session" in cookie_names, f"michi_web_session cookie not found in jar: {cookie_names}"
    return opener, cj, True


class TestWebUIStaticAssets:
    def test_root_and_static_html(self):
        req = urllib.request.Request(f"{SERVER_URL}/")
        with urllib.request.urlopen(req, timeout=5) as resp:
            assert resp.status == 200
            body = resp.read().decode("utf-8")
            assert "Michi Micro Server" in body
            assert "styles.css?v=" in body
            assert "app.js?v=" in body
            assert "Checking..." in body
            assert '<span class="status-pill" id="status-pill"><span class="server-status-dot"></span>Online</span>' not in body

    def test_sw_js_and_manifest(self):
        with urllib.request.urlopen(f"{SERVER_URL}/sw.js", timeout=5) as resp:
            assert resp.status == 200
            body = resp.read().decode("utf-8")
            assert "michi-v" in body
            assert "app.js?v=" in body

        with urllib.request.urlopen(f"{SERVER_URL}/manifest.json", timeout=5) as resp:
            assert resp.status == 200
            data = json.loads(resp.read().decode("utf-8"))
            assert data["name"] == "Michi Micro Server"


class TestWebUIFunctionalConformance:
    def test_status_endpoint_truthfulness(self):
        with urllib.request.urlopen(f"{SERVER_URL}/api/v1/status", timeout=5) as resp:
            assert resp.status == 200
            data = json.loads(resp.read().decode("utf-8"))
            assert data["status"] == "ok"
            assert "service" in data
            assert "database" in data
            assert "music_paths" in data

    def test_server_info_capabilities_and_dashboard(self):
        with urllib.request.urlopen(f"{SERVER_URL}/api/v1/server/info", timeout=5) as resp:
            assert resp.status == 200
            info = json.loads(resp.read().decode("utf-8"))
            assert info["service"] == "michi-micro-server"
            assert "features" in info

        # Unauthenticated request to protected dashboard should fail if auth is enabled
        unauth_opener, _ = _get_opener()
        auth_opener, _, auth_enabled = _get_authenticated_opener()

        if auth_enabled:
            with pytest.raises(urllib.error.HTTPError) as excinfo:
                unauth_opener.open(f"{SERVER_URL}/api/v1/home/dashboard", timeout=5)
            assert excinfo.value.code == 401, f"Expected 401 without auth, got {excinfo.value.code}"

        # Authenticated request to dashboard must succeed
        with auth_opener.open(f"{SERVER_URL}/api/v1/home/dashboard", timeout=5) as resp:
            assert resp.status == 200
            dash = json.loads(resp.read().decode("utf-8"))
            assert "library" in dash
            assert "health" in dash
            assert "playback" in dash

    def test_playlists_crud_lifecycle(self):
        auth_opener, _, _ = _get_authenticated_opener()

        # 1. Create playlist
        create_payload = json.dumps({"name": "E2E Test Playlist", "description": "Conformance test"}).encode("utf-8")
        req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/playlists",
            data=create_payload,
            headers={"Content-Type": "application/json"},
            method="POST"
        )
        with auth_opener.open(req, timeout=5) as resp:
            assert resp.status == 200
            data = json.loads(resp.read().decode("utf-8"))
            playlist = data.get("playlist", data)
            playlist_id = playlist["id"]
            assert playlist["name"] == "E2E Test Playlist"

        # 2. Update playlist
        update_payload = json.dumps({"name": "E2E Test Playlist Renamed"}).encode("utf-8")
        req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/playlists/{playlist_id}",
            data=update_payload,
            headers={"Content-Type": "application/json"},
            method="PUT"
        )
        with auth_opener.open(req, timeout=5) as resp:
            assert resp.status == 200
            data = json.loads(resp.read().decode("utf-8"))
            assert data.get("playlist", {}).get("name") == "E2E Test Playlist Renamed" or data.get("status") == "ok"

        # 3. Get playlist tracks
        req = urllib.request.Request(f"{SERVER_URL}/api/v1/playlists/{playlist_id}/tracks")
        with auth_opener.open(req, timeout=5) as resp:
            assert resp.status == 200
            data = json.loads(resp.read().decode("utf-8"))
            assert "tracks" in data

        # 4. Export M3U
        req = urllib.request.Request(f"{SERVER_URL}/api/v1/playlists/{playlist_id}/export/m3u")
        with auth_opener.open(req, timeout=5) as resp:
            assert resp.status == 200
            assert resp.headers.get_content_type() == "audio/x-mpegurl"

        # 5. Delete playlist
        req = urllib.request.Request(
            f"{SERVER_URL}/api/v1/playlists/{playlist_id}",
            method="DELETE"
        )
        with auth_opener.open(req, timeout=5) as resp:
            assert resp.status == 200

    def test_canonical_v1_routes_availability(self):
        auth_opener, _, _ = _get_authenticated_opener()

        # Diagnostics
        req = urllib.request.Request(f"{SERVER_URL}/api/v1/diagnostics")
        with auth_opener.open(req, timeout=5) as resp:
            assert resp.status == 200

        # Health
        for path in ["/api/v1/health/mounts", "/api/v1/health/storage", "/api/v1/health/self-test"]:
            req = urllib.request.Request(f"{SERVER_URL}{path}")
            with auth_opener.open(req, timeout=5) as resp:
                assert resp.status in (200, 503)

        # Changes
        req = urllib.request.Request(f"{SERVER_URL}/api/v1/changes")
        with auth_opener.open(req, timeout=5) as resp:
            assert resp.status == 200
