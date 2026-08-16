#!/usr/bin/env python3
"""
Player-Micro Server Contract Compatibility Test.

Tests all endpoints that Michi Music Player consumes against
a running Michi Micro Server instance.

Usage:
  python3 test_player_micro_contract_compatibility.py [--url http://localhost:8096] [--username admin] [--password admin]
"""

import argparse
import json
import os
import sys
import urllib.request
import urllib.error

DEFAULT_URL = "http://127.0.0.1:8096"
FIXTURES_DIR = os.path.join(os.path.dirname(__file__), "..", "fixtures", "micro_contract")

PASS = 0
FAIL = 0
SKIP = 0


def load_fixture(name):
    path = os.path.join(FIXTURES_DIR, name)
    with open(path) as f:
        return json.load(f)


def test(base_url, name, method, path, expected_status=200, body=None, headers=None):
    global PASS, FAIL
    url = f"{base_url}{path}"
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        resp = urllib.request.urlopen(req, timeout=5)
        status = resp.status
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
    parser.add_argument("--password", default=os.getenv("MICHI_AUTH_PASSWORD", "admin"), help="Auth password")
    args = parser.parse_args()

    base_url = args.url.rstrip("/")

    print(f"\n{'='*60}")
    print(f"Player-Micro Server Contract Compatibility Test")
    print(f"Target: {base_url}")
    print(f"{'='*60}\n")

    # 1. Server info (Public)
    print("[1] Server Info")
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

    # 2. Authenticate if required or available
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
            print("  ⚠️ Could not authenticate with provided credentials; trying unauthenticated")

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

    # 5. Queue transfer
    print("\n[5] Queue Transfer")
    test(
        base_url,
        "POST /api/v1/queue/transfer validation",
        "POST",
        "/api/v1/queue/transfer",
        expected_status=400,
        body={"track_ids": [], "current_index": 0, "position_ms": 0, "source": "test"},
        headers=auth_headers,
    )

    # 6. Diagnostics
    print("\n[6] Diagnostics")
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

    # 7. Playback state
    print("\n[7] Playback State")
    state = test(
        base_url,
        "GET /api/v1/playback/state",
        "GET",
        "/api/v1/playback/state",
        headers=auth_headers,
    )
    if state:
        assert "state" in state
        assert "position_ms" in state
        assert "volume" in state

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
