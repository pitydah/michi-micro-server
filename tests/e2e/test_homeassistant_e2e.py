#!/usr/bin/env python3
"""
Home Assistant & MQTT Real E2E Integration Test.

Validates MQTT Auto-Discovery, State Publication, Command Processing,
and Broker Disconnect/Reconnect resilience with Michi Micro Server.

Usage:
  python3 tests/e2e/test_homeassistant_e2e.py --admin-url http://127.0.0.1:18884 --server-url http://127.0.0.1:9099
"""

import argparse
import json
import sys
import time
import urllib.request
import urllib.error

PASS = 0
FAIL = 0

AUTH_TOKEN = None

def http_get(url):
    headers = {}
    if AUTH_TOKEN:
        headers["Authorization"] = f"Bearer {AUTH_TOKEN}"
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=5) as resp:
        return json.loads(resp.read().decode("utf-8"))

def http_post(url, payload=None):
    headers = {"Content-Type": "application/json"}
    if AUTH_TOKEN:
        headers["Authorization"] = f"Bearer {AUTH_TOKEN}"
    data = json.dumps(payload or {}).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=5) as resp:
        return json.loads(resp.read().decode("utf-8"))

def test(name, func):
    global PASS, FAIL
    try:
        func()
        print(f"  ✅ {name}")
        PASS += 1
    except Exception as e:
        print(f"  ❌ {name}: {e}")
        FAIL += 1

def main():
    global AUTH_TOKEN
    parser = argparse.ArgumentParser(description="Home Assistant & MQTT E2E Test")
    parser.add_argument("--admin-url", default="http://127.0.0.1:18884")
    parser.add_argument("--server-url", default="http://127.0.0.1:9099")
    parser.add_argument("--username", default="admin")
    parser.add_argument("--password", default="TestAdminPassword123!")
    args = parser.parse_args()

    admin_url = args.admin_url.rstrip("/")
    server_url = args.server_url.rstrip("/")

    print("=" * 60)
    print("Home Assistant & MQTT Real E2E Integration Test")
    print(f"Admin API: {admin_url} | Micro Server: {server_url}")
    print("=" * 60)

    # Attempt login to obtain token if server requires auth
    try:
        login_resp = http_post(f"{server_url}/api/auth/login", {
            "username": args.username,
            "password": args.password,
        })
        AUTH_TOKEN = login_resp.get("token")
    except Exception:
        pass

    # 1. Verify Auto-Discovery Messages Received
    def test_discovery():
        # Wait up to 10 seconds for discovery messages
        for _ in range(20):
            res = http_get(f"{admin_url}/api/mqtt/messages")
            topics = [m["topic"] for m in res.get("messages", [])]
            if any("homeassistant/sensor/michi_track_title/config" in t for t in topics):
                break
            time.sleep(0.5)

        res = http_get(f"{admin_url}/api/mqtt/messages")
        msgs = {m["topic"]: m["payload"] for m in res.get("messages", [])}

        required_discovery = [
            "homeassistant/sensor/michi_track_title/config",
            "homeassistant/sensor/michi_artist/config",
            "homeassistant/sensor/michi_album/config",
            "homeassistant/sensor/michi_playback_status/config",
            "homeassistant/sensor/michi_volume/config",
            "homeassistant/sensor/michi_server_status/config",
            "homeassistant/button/michi_play_pause/config",
            "homeassistant/number/michi_volume_set/config",
        ]
        for topic in required_discovery:
            assert topic in msgs, f"missing discovery topic: {topic}"
            config = json.loads(msgs[topic])
            assert "name" in config
            assert "unique_id" in config
    test("MQTT Auto-Discovery Topics (Sensors, Buttons, Numbers)", test_discovery)

    # 2. Verify Periodic State Publication
    def test_state_publication():
        res = http_get(f"{admin_url}/api/mqtt/messages")
        msgs = {m["topic"]: m["payload"] for m in res.get("messages", [])}

        assert "michi/server_status/state" in msgs, "missing michi/server_status/state"
        assert msgs["michi/server_status/state"] == "online"

        assert "michi/playback_status/state" in msgs, "missing michi/playback_status/state"
        assert msgs["michi/playback_status/state"] in ("playing", "paused")

        assert "michi/volume/state" in msgs, "missing michi/volume/state"
    test("Periodic State Publication (server_status, playback_status, volume)", test_state_publication)

    # 3. Test Command Processing (play_pause without output fails-closed to paused)
    def test_command_play_pause():
        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/play_pause/cmd",
            "payload": ""
        })
        time.sleep(0.5)
        # Verify server state endpoint (must remain paused without output selected)
        state = http_get(f"{server_url}/api/v1/playback/state")
        assert "playing" in state, f"expected playing in state: {state}"
        assert state["playing"] is False, f"expected playing to be false without output, got {state['playing']}"
    test("Incoming MQTT Command Handling (michi/play_pause/cmd fail-closed)", test_command_play_pause)

    # 4. Test Volume Set via MQTT
    def test_command_volume_set():
        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/volume_set/cmd",
            "payload": "75"
        })
        time.sleep(0.5)
        state = http_get(f"{server_url}/api/v1/playback/state")
        assert state.get("volume") == 75, f"expected volume 75, got {state.get('volume')}"
    test("Incoming MQTT Volume Set (michi/volume_set/cmd)", test_command_volume_set)

    # 5. Broker Disconnect & Auto-Reconnect Resilience
    def test_broker_disconnect_reconnect():
        # Drop all connections
        http_post(f"{admin_url}/api/mqtt/drop")
        time.sleep(2.0)

        # Clear message history
        http_post(f"{admin_url}/api/mqtt/clear")

        # Restore broker
        http_post(f"{admin_url}/api/mqtt/restore")

        # Wait up to 10s for reconnect & new messages
        reconnected = False
        for _ in range(20):
            res = http_get(f"{admin_url}/api/mqtt/messages")
            if len(res.get("messages", [])) > 0:
                reconnected = True
                break
            time.sleep(0.5)

        assert reconnected, "server failed to auto-reconnect to MQTT broker after network recovery"
    test("Broker Disconnect & Auto-Reconnect Resilience", test_broker_disconnect_reconnect)

    # Summary
    print("\n" + "=" * 60)
    print(f"Home Assistant & MQTT E2E Results: {PASS} passed, {FAIL} failed ({PASS + FAIL} total)")
    if FAIL > 0:
        sys.exit(1)
    else:
        print("HOME ASSISTANT E2E: PASS")
        sys.exit(0)

if __name__ == "__main__":
    main()
