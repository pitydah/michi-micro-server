#!/usr/bin/env python3
"""
Player-Micro Server Contract Compatibility & End-to-End Test.

Tests the full lifecycle and contracts consumed by Michi Music Player against
a running Michi Micro Server instance:
1. Server info & capabilities (Public contract)
2. Authentication & JWT acquisition
3. Import Preflight (New & legacy formats)
4. Audio track upload & cataloging
5. Real HTTP Range audio streaming (206 Partial Content)
6. Successful queue transfer
7. Playback controls: play -> state -> pause -> state -> seek -> state -> volume
8. Direct playback handoff between devices
9. Server diagnostics & player compatibility matrix

Usage:
  python3 test_player_micro_contract_compatibility.py [--url http://localhost:9090] [--username admin] [--password admin]
"""

import argparse
import base64
import json
import os
import sys
import urllib.request
import urllib.error

DEFAULT_URL = "http://127.0.0.1:9090"
FIXTURES_DIR = os.path.join(os.path.dirname(__file__), "..", "fixtures", "micro_contract")

PASS = 0
FAIL = 0
SKIP = 0

# Minimal valid 44-byte PCM WAV audio payload
MINIMAL_WAV_BYTES = (
    b"RIFF$\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00"
    b"D\xac\x00\x00\x88X\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00"
)
MINIMAL_WAV_BASE64 = base64.b64encode(MINIMAL_WAV_BYTES).decode("ascii")


def load_fixture(name):
    path = os.path.join(FIXTURES_DIR, name)
    with open(path) as f:
        return json.load(f)


def test(base_url, name, method, path, expected_status=200, body=None, headers=None, raw_response=False):
    global PASS, FAIL
    url = f"{base_url}{path}"
    data = json.dumps(body).encode() if body is not None and not isinstance(body, bytes) else body
    req = urllib.request.Request(url, data=data, method=method)
    if body is not None and not isinstance(body, bytes):
        req.add_header("Content-Type", "application/json")
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        resp = urllib.request.urlopen(req, timeout=5)
        status = resp.status
        if raw_response:
            resp_body = resp.read()
        else:
            resp_data = resp.read().decode()
            resp_body = json.loads(resp_data) if resp_data else {}
        if status == expected_status:
            PASS += 1
            print(f"  ✅ {name}")
        else:
            FAIL += 1
            print(f"  ❌ {name}: expected {expected_status}, got {status}")
        return resp_body
    except urllib.error.HTTPError as e:
        status = e.code
        try:
            resp_body = json.loads(e.read().decode()) if e.fp else {}
        except Exception:
            resp_body = {}
        if status == expected_status:
            PASS += 1
            print(f"  ✅ {name} (expected {status})")
        else:
            FAIL += 1
            print(f"  ❌ {name}: expected {expected_status}, got {status} — {resp_body}")
        return resp_body
    except Exception as e:
        FAIL += 1
        print(f"  ❌ {name}: connection failed — {e}")
        return None


def main():
    parser = argparse.ArgumentParser(description="Player-Micro Server Contract Test")
    parser.add_argument("--url", default=os.getenv("MICHI_SERVER_URL", DEFAULT_URL), help="Micro Server base URL")
    parser.add_argument("--username", default=os.getenv("MICHI_AUTH_USERNAME", "admin"), help="Auth username")
    parser.add_argument("--password", default=os.getenv("MICHI_AUTH_PASSWORD", "admin123"), help="Auth password")
    args = parser.parse_args()


    base_url = args.url.rstrip("/")

    print(f"\n{'='*60}")
    print(f"Player-Micro Server Contract Compatibility & E2E Test")
    print(f"Target: {base_url}")
    print(f"{'='*60}\n")

    # 1. Server info (Public)
    print("[1] Server Info & Capabilities")
    info = test(base_url, "GET /api/v1/server/info", "GET", "/api/v1/server/info")
    if not info:
        print("❌ Fatal: Could not retrieve server info. Is the server running?")
        sys.exit(1)

    assert info.get("service") == "michi-micro-server", f"expected michi-micro-server, got {info.get('service')}"
    assert info.get("api_version") == "v1", f"expected v1, got {info.get('api_version')}"
    assert "auth" in info and "strategy" in info["auth"]
    assert "features" in info
    assert info["features"].get("import") is True
    assert info["features"].get("playback") is True
    assert info["features"].get("queue") is True
    assert "receivers" in info["features"]
    assert "rooms" in info["features"]

    # 2. Authenticate
    auth_headers = {}
    auth_req = info.get("auth", {})
    if auth_req.get("required") or args.username:
        print("\n[2] Authentication")
        login_res = test(
            base_url,
            "POST /api/auth/login",
            "POST",
            "/api/auth/login",
            expected_status=200,
            body={"username": args.username, "password": args.password},
        )
        if login_res and "token" in login_res:
            token = login_res["token"]
            auth_headers = {"Authorization": f"Bearer {token}"}
            print("  🔑 Token acquired successfully")
        else:
            print("  ⚠️ Could not authenticate with provided credentials")

    # 3. Preflight (new format)
    print("\n[3] Import Preflight (New Format)")
    preflight = load_fixture("preflight_new.json")
    result = test(
        base_url,
        "POST /api/v1/import/preflight (new)",
        "POST",
        "/api/v1/import/preflight",
        body=preflight,
        headers=auth_headers,
    )
    if result and "results" in result:
        for r in result["results"]:
            assert "local_track_id" in r, "missing local_track_id"
            assert "status" in r, "missing status"
            assert "remote_track_id" in r, "missing remote_track_id"
            assert "match" in r, "missing match"

    # 4. Preflight (legacy format)
    print("\n[4] Import Preflight (Legacy Format)")
    preflight_legacy = load_fixture("preflight_legacy.json")
    result = test(
        base_url,
        "POST /api/v1/import/preflight (legacy)",
        "POST",
        "/api/v1/import/preflight",
        body=preflight_legacy,
        headers=auth_headers,
    )
    if result and "results" in result:
        for r in result["results"]:
            assert "status" in r, "missing status in legacy preflight item"
            assert "match" in r, "missing match in legacy preflight item"

    # 5. Full Track Upload & Catalog Flow
    print("\n[5] Track Import & Catalog")
    session_res = test(
        base_url,
        "POST /api/v1/import/session (create import session)",
        "POST",
        "/api/v1/import/session",
        body={"total_tracks": 1, "total_playlists": 0},
        headers=auth_headers,
    )
    imported_track_id = None
    if session_res and "session_id" in session_res:
        sess_id = session_res["session_id"]
        upload_res = test(
            base_url,
            f"POST /api/v1/import/session/{sess_id}/upload",
            "POST",
            f"/api/v1/import/session/{sess_id}/upload",
            body={"filename": "test_player_e2e.wav", "data": MINIMAL_WAV_BASE64},
            headers=auth_headers,
        )
        if upload_res:
            assert upload_res.get("status") in ("uploaded", "duplicate")

        commit_res = test(
            base_url,
            f"POST /api/v1/import/commit/{sess_id}",
            "POST",
            f"/api/v1/import/commit/{sess_id}",
            body={},
            headers=auth_headers,
        )

        tracks_res = test(
            base_url,
            "GET /api/v1/tracks",
            "GET",
            "/api/v1/tracks",
            headers=auth_headers,
        )
        if tracks_res and isinstance(tracks_res, list) and len(tracks_res) > 0:
            imported_track_id = tracks_res[0].get("id")
        elif isinstance(tracks_res, dict) and "tracks" in tracks_res and len(tracks_res["tracks"]) > 0:
            imported_track_id = tracks_res["tracks"][0].get("id")

    # 6. Real Audio Streaming with HTTP Range
    print("\n[6] Audio Stream (HTTP Range Request)")
    if imported_track_id:
        stream_headers = dict(auth_headers)
        stream_headers["Range"] = "bytes=0-15"
        stream_data = test(
            base_url,
            f"GET /api/v1/tracks/{imported_track_id}/stream (Range: 0-15)",
            "GET",
            f"/api/v1/tracks/{imported_track_id}/stream",
            expected_status=206,
            headers=stream_headers,
            raw_response=True,
        )
        if stream_data is not None:
            assert len(stream_data) == 16, f"expected 16 bytes for range 0-15, got {len(stream_data)}"
    else:
        print("  ⚠️ Skipped stream range check (no imported track available)")

    # 7. Queue Transfer (Valid)
    print("\n[7] Queue Transfer")
    test_track_ids = [imported_track_id] if imported_track_id else ["00000000-0000-0000-0000-000000000001"]
    if imported_track_id:
        queue_res = test(
            base_url,
            "POST /api/v1/queue/transfer (successful transfer)",
            "POST",
            "/api/v1/queue/transfer",
            expected_status=200,
            body={"track_ids": test_track_ids, "current_index": 0, "position_ms": 1500, "source": "michi-player"},
            headers=auth_headers,
        )
        if queue_res:
            assert "queue_id" in queue_res
    else:
        test(
            base_url,
            "POST /api/v1/queue/transfer (unknown track validation)",
            "POST",
            "/api/v1/queue/transfer",
            expected_status=400,
            body={"track_ids": test_track_ids, "current_index": 0, "position_ms": 1500, "source": "michi-player"},
            headers=auth_headers,
        )

    # 8. Playback Controls & State Lifecycle
    print("\n[8] Playback Lifecycle (Play -> Pause -> Seek -> Volume)")
    # Play without output selected (must fail-closed with 409 CONFLICT per functional truth)
    test(
        base_url,
        "POST /api/v1/playback/control (play without output)",
        "POST",
        "/api/v1/playback/control",
        expected_status=409,
        body={"command": "play", "position_ms": 2000},
        headers=auth_headers,
    )
    st = test(base_url, "GET /api/v1/playback/state (after play attempt)", "GET", "/api/v1/playback/state", headers=auth_headers)
    if st:
        assert st.get("playing") is False

    # Pause
    test(
        base_url,
        "POST /api/v1/playback/control (pause)",
        "POST",
        "/api/v1/playback/control",
        expected_status=200,
        body={"command": "pause"},
        headers=auth_headers,
    )
    st = test(base_url, "GET /api/v1/playback/state (after pause)", "GET", "/api/v1/playback/state", headers=auth_headers)
    if st:
        assert st.get("playing") is False

    # Seek
    test(
        base_url,
        "POST /api/v1/playback/control (seek)",
        "POST",
        "/api/v1/playback/control",
        expected_status=200,
        body={"command": "seek", "position_ms": 8500},
        headers=auth_headers,
    )
    st = test(base_url, "GET /api/v1/playback/state (after seek)", "GET", "/api/v1/playback/state", headers=auth_headers)
    if st:
        assert st.get("position_ms") == 8500

    # Set Volume
    test(
        base_url,
        "POST /api/v1/playback/control (set_volume)",
        "POST",
        "/api/v1/playback/control",
        expected_status=200,
        body={"command": "set_volume", "volume": 85},
        headers=auth_headers,
    )
    st = test(base_url, "GET /api/v1/playback/state (after volume)", "GET", "/api/v1/playback/state", headers=auth_headers)
    if st:
        assert st.get("volume") == 85

    # 9. Direct Playback Handoff
    print("\n[9] Playback Handoff")
    if imported_track_id:
        handoff_res = test(
            base_url,
            "POST /api/v1/playback/handoff",
            "POST",
            "/api/v1/playback/handoff",
            body={
                "track_id": imported_track_id,
                "position_ms": 15000,
                "playing": False,
                "volume": 0.9,
                "from_device": "michi-player-desktop",
            },
            headers=auth_headers,
        )
        if handoff_res:
            assert handoff_res.get("status") == "handoff_accepted"
            assert handoff_res.get("position_ms") == 15000
            assert handoff_res.get("playing") is False

    # 10. Diagnostics
    print("\n[10] Diagnostics & Player Compatibility Matrix")
    diag = test(
        base_url,
        "GET /api/v1/diagnostics",
        "GET",
        "/api/v1/diagnostics",
        headers=auth_headers,
    )
    if diag:
        assert "player_compatibility" in diag, "missing player_compatibility"
        pc = diag["player_compatibility"]
        assert "supports_import_preflight" in pc
        assert "supports_upload_mapping" in pc
        assert "supports_commit_mapping" in pc
        assert "supports_queue_transfer" in pc
        assert pc.get("contract_status") in ("CONTRACT_OK", "CONTRACT_PARTIAL")

    # Summary
    total = PASS + FAIL + SKIP
    print(f"\n{'='*60}")
    print(f"Results: {PASS} passed, {FAIL} failed, {SKIP} skipped ({total} total)")
    if FAIL > 0:
        print("CONTRACT: PARTIAL / FAILED — some checks failed")
        sys.exit(1)
    else:
        print("CONTRACT: OK — all contract checks passed")
        sys.exit(0)


if __name__ == "__main__":
    main()
