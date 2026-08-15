#!/usr/bin/env python3
"""
Player-Micro Server Contract Compatibility Test.

Verifies every endpoint that Michi Music Player consumes against a running
Michi Micro Server instance, following the authenticated admin flow:

  1. GET  public /api/v1/server/info (no Authorization header)
  2. POST /api/auth/login with configured throwaway admin credentials
  3. Require the returned token
  4. Send `Authorization: Bearer <token>` on every protected request
     (import preflight, queue transfer, diagnostics, playback state)

Missing credentials, login failure, an absent/invalid token, or any protected
401 MUST record a FAIL and exit non-zero. Nothing is skipped.

Usage:
  MICHI_AUTH_USERNAME=admin MICHI_AUTH_PASSWORD=... \
    python3 test_player_micro_contract_compatibility.py --url http://127.0.0.1:8096
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

DEFAULT_BASE_URL = "http://127.0.0.1:8096"
FIXTURES_DIR = os.path.join(os.path.dirname(__file__), "..", "fixtures", "micro_contract")

PASS = 0
FAIL = 0


def load_fixture(name):
    path = os.path.join(FIXTURES_DIR, name)
    with open(path) as f:
        return json.load(f)


def test(name, method, path, base_url, expected_status=200, body=None, headers=None, timeout=5):
    """Issue one request against base_url and record PASS/FAIL. Returns parsed body or None."""
    global PASS, FAIL
    url = f"{base_url}{path}"
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        resp = urllib.request.urlopen(req, timeout=timeout)
        status = resp.status
        resp_body = json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        status = e.code
        resp_body = json.loads(e.read().decode()) if e.fp else {}
    except Exception as e:
        FAIL += 1
        print(f"  ❌ {name}: connection failed — {e}")
        return None
    if status == expected_status:
        PASS += 1
        print(f"  ✅ {name}")
    else:
        FAIL += 1
        print(f"  ❌ {name}: expected {expected_status}, got {status} — {resp_body}")
    return resp_body


def verify(condition, message):
    """Record a PASS or FAIL for a field assertion. Never skips."""
    global PASS, FAIL
    if condition:
        PASS += 1
        print(f"  ✅ {message}")
    else:
        FAIL += 1
        print(f"  ❌ {message}")


def summary():
    total = PASS + FAIL
    print(f"\n{'=' * 60}")
    print(f"Results: {PASS} passed, {FAIL} failed ({total} total)")
    if FAIL > 0:
        print("CONTRACT: FAILED — one or more checks failed")
    else:
        print("CONTRACT: OK")


def run_contract(base_url, username, password):
    global FAIL

    if not username or not password:
        FAIL += 1
        print(
            "  ❌ admin credentials not configured — set MICHI_AUTH_USERNAME and MICHI_AUTH_PASSWORD"
        )
        summary()
        sys.exit(1)

    # 1. Server info (public — no Authorization header)
    print("[1] Server Info (public)")
    info = test("GET /api/v1/server/info", "GET", "/api/v1/server/info", base_url)
    body = info or {}
    verify(body.get("service") == "michi-micro-server", "server info: service == 'michi-micro-server'")
    verify(body.get("api_version") == "v1", "server info: api_version == 'v1'")
    auth = body.get("auth") or {}
    verify(auth.get("strategy") == "SERVER_CODE", "server info: auth.strategy == 'SERVER_CODE'")
    verify(auth.get("token_refresh") is True, "server info: auth.token_refresh == true")
    features = body.get("features") or {}
    verify(features.get("import") is True, "server info: features.import == true")
    verify(features.get("playback") is True, "server info: features.playback == true")
    verify(features.get("queue") is True, "server info: features.queue == true")

    # 2. Admin login
    print("\n[2] Admin Login")
    login = test(
        "POST /api/auth/login",
        "POST",
        "/api/auth/login",
        base_url,
        body={"username": username, "password": password},
    )
    token = (login or {}).get("token")
    verify(bool(token), "login returned a token")
    if not token:
        FAIL += 1
        print("  ❌ no token returned — cannot proceed to protected checks")
        summary()
        sys.exit(1)

    auth_headers = {"Authorization": f"Bearer {token}"}

    # 3. Import preflight (new format)
    print("\n[3] Import Preflight (new format)")
    preflight = load_fixture("preflight_new.json")
    result = test(
        "POST /api/v1/import/preflight (new)",
        "POST",
        "/api/v1/import/preflight",
        base_url,
        body=preflight,
        headers=auth_headers,
    )
    rbody = result or {}
    results = rbody.get("results")
    if results:
        for i, r in enumerate(results):
            verify("local_track_id" in r, f"preflight new result[{i}]: local_track_id present")
            verify("status" in r, f"preflight new result[{i}]: status present")
            verify("remote_track_id" in r, f"preflight new result[{i}]: remote_track_id present")
            verify("match" in r, f"preflight new result[{i}]: match present")
    else:
        verify(False, "preflight (new): response missing 'results'")

    # 4. Import preflight (legacy format)
    print("\n[4] Import Preflight (legacy format)")
    preflight_legacy = load_fixture("preflight_legacy.json")
    result = test(
        "POST /api/v1/import/preflight (legacy)",
        "POST",
        "/api/v1/import/preflight",
        base_url,
        body=preflight_legacy,
        headers=auth_headers,
    )
    rbody = result or {}
    results = rbody.get("results")
    if results:
        for i, r in enumerate(results):
            verify("status" in r, f"preflight legacy result[{i}]: status present")
            verify("match" in r, f"preflight legacy result[{i}]: match present")
    else:
        verify(False, "preflight (legacy): response missing 'results'")

    # 5. Queue transfer (empty body → documented 400)
    print("\n[5] Queue Transfer (empty body → 400)")
    test(
        "POST /api/v1/queue/transfer (empty body → 400)",
        "POST",
        "/api/v1/queue/transfer",
        base_url,
        expected_status=400,
        body={"track_ids": [], "current_index": 0, "position_ms": 0, "source": "test"},
        headers=auth_headers,
    )

    # 6. Diagnostics player_compatibility
    print("\n[6] Diagnostics")
    diag = test("GET /api/v1/diagnostics", "GET", "/api/v1/diagnostics", base_url, headers=auth_headers)
    dbody = diag or {}
    verify("player_compatibility" in dbody, "diagnostics: player_compatibility present")
    pc = dbody.get("player_compatibility") or {}
    # supports_* are runtime-state flags (active import sessions / queues present),
    # not static capabilities — assert the block shape, not a True value.
    verify("supports_import_preflight" in pc, "diagnostics: supports_import_preflight field present")
    verify("supports_upload_mapping" in pc, "diagnostics: supports_upload_mapping field present")
    verify("supports_commit_mapping" in pc, "diagnostics: supports_commit_mapping field present")
    verify("supports_queue_transfer" in pc, "diagnostics: supports_queue_transfer field present")
    verify(
        pc.get("contract_status") in ("CONTRACT_OK", "CONTRACT_PARTIAL"),
        "diagnostics: contract_status in (CONTRACT_OK, CONTRACT_PARTIAL)",
    )

    # 7. Playback state shape
    print("\n[7] Playback State")
    state = test("GET /api/v1/playback/state", "GET", "/api/v1/playback/state", base_url, headers=auth_headers)
    sbody = state or {}
    verify("state" in sbody, "playback state: 'state' present")
    verify("track_id" in sbody, "playback state: 'track_id' present")
    verify("position_ms" in sbody, "playback state: 'position_ms' present")
    verify("volume" in sbody, "playback state: 'volume' present")

    summary()
    sys.exit(1 if FAIL > 0 else 0)


def main():
    parser = argparse.ArgumentParser(description="Player-Micro Server Contract Test")
    parser.add_argument("--url", default=DEFAULT_BASE_URL, help="Micro Server base URL")
    args = parser.parse_args()
    base_url = args.url.rstrip("/")
    username = os.environ.get("MICHI_AUTH_USERNAME")
    password = os.environ.get("MICHI_AUTH_PASSWORD")

    print(f"\n{'=' * 60}")
    print("Player-Micro Server Contract Compatibility Test")
    print(f"Target: {base_url}")
    print(f"{'=' * 60}")

    run_contract(base_url, username, password)


if __name__ == "__main__":
    main()
