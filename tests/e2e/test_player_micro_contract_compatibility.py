#!/usr/bin/env python3
"""
Player-Micro Server Contract Compatibility Test.

Tests all endpoints that Michi Music Player consumes against
a running Michi Micro Server instance.

Usage:
  python3 test_player_micro_contract_compatibility.py [--url http://localhost:8096]
"""

import argparse
import json
import os
import sys
import urllib.request
import urllib.error

BASE_URL = "http://127.0.0.1:8096"
FIXTURES_DIR = os.path.join(os.path.dirname(__file__), "..", "fixtures", "micro_contract")

PASS = 0
FAIL = 0
SKIP = 0

def load_fixture(name):
    path = os.path.join(FIXTURES_DIR, name)
    with open(path) as f:
        return json.load(f)

def test(name, method, path, expected_status=200, body=None, headers=None):
    global PASS, FAIL
    url = f"{BASE_URL}{path}"
    data = json.dumps(body).encode() if body else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        resp = urllib.request.urlopen(req, timeout=5)
        status = resp.status
        resp_body = json.loads(resp.read().decode())
        if status == expected_status:
            PASS += 1
            print(f"  ✅ {name}")
        else:
            FAIL += 1
            print(f"  ❌ {name}: expected {expected_status}, got {status}")
        return resp_body
    except urllib.error.HTTPError as e:
        status = e.code
        resp_body = json.loads(e.read().decode()) if e.fp else {}
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
    global SKIP
    parser = argparse.ArgumentParser(description="Player-Micro Server Contract Test")
    parser.add_argument("--url", default=BASE_URL, help="Micro Server base URL")
    args = parser.parse_args()
    global BASE_URL
    BASE_URL = args.url.rstrip("/")

    print(f"\n{'='*60}")
    print(f"Player-Micro Server Contract Compatibility Test")
    print(f"Target: {BASE_URL}")
    print(f"{'='*60}\n")

    # 1. Server info
    print("[1] Server Info")
    info = test("GET /api/v1/server/info", "GET", "/api/v1/server/info")
    if info:
        assert info.get("service") == "michi-micro-server", f"expected michi-micro-server, got {info.get('service')}"
        assert info.get("michi_link_version") == "1.0.0-alpha"
        assert info["auth"]["strategy"] == "SERVER_CODE"
        assert info["auth"]["token_refresh"] == True
        assert info["features"]["import"] == True
        assert info["features"]["playback"] == True
        assert info["features"]["queue"] == True

    # 2. Preflight (new format)
    print("\n[2] Import Preflight")
    preflight = load_fixture("preflight_new.json")
    result = test("POST /api/v1/import/preflight (new)", "POST", "/api/v1/import/preflight",
                  body=preflight)
    if result and "results" in result:
        for r in result["results"]:
            assert "local_track_id" in r, "missing local_track_id"
            assert "status" in r, "missing status"
            assert "remote_track_id" in r, "missing remote_track_id"
            assert "match" in r, "missing match"

    # 3. Preflight (legacy format)
    preflight_legacy = load_fixture("preflight_legacy.json")
    result = test("POST /api/v1/import/preflight (legacy)", "POST", "/api/v1/import/preflight",
                  body=preflight_legacy)
    if result and "results" in result:
        for r in result["results"]:
            assert "status" in r
            assert "match" in r

    # 4. Queue transfer
    print("\n[3] Queue Transfer")
    # Need to seed a track first — skip validation, just test endpoint existence
    test("POST /api/v1/queue/transfer exists", "POST", "/api/v1/queue/transfer",
         expected_status=400, body={"track_ids": [], "current_index": 0, "position_ms": 0, "source": "test"})

    # 5. Diagnostics
    print("\n[4] Diagnostics")
    diag = test("GET /api/v1/diagnostics", "GET", "/api/v1/diagnostics")
    if diag:
        assert "player_compatibility" in diag, "missing player_compatibility"
        pc = diag["player_compatibility"]
        assert pc["supports_import_preflight"] == True
        assert pc["supports_upload_mapping"] == True
        assert pc["supports_commit_mapping"] == True
        assert pc["supports_queue_transfer"] == True
        assert pc["contract_status"] in ("CONTRACT_OK", "CONTRACT_PARTIAL")

    # 6. Playback state
    print("\n[5] Playback State")
    state = test("GET /api/v1/playback/state", "GET", "/api/v1/playback/state")
    if state:
        assert "state" in state
        assert "track_id" in state
        assert "position_ms" in state
        assert "volume" in state

    # Summary
    total = PASS + FAIL + SKIP
    print(f"\n{'='*60}")
    print(f"Results: {PASS} passed, {FAIL} failed, {SKIP} skipped ({total} total)")
    if FAIL > 0:
        print("CONTRACT: PARTIAL — some checks failed")
        sys.exit(1)
    elif SKIP > 0:
        print("CONTRACT: PARTIAL — some checks were skipped")
        sys.exit(0)
    else:
        print("CONTRACT: OK")
        sys.exit(0)

if __name__ == "__main__":
    main()
