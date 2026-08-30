#!/usr/bin/env python3
"""
Home Assistant & MQTT Real E2E Integration Test.

Validates MQTT Auto-Discovery, State Publication, Command Processing,
Direct SQLite PlaybackSession Persistence, and Broker Disconnect/Reconnect
resilience with Michi Micro Server.

Usage:
  python3 tests/e2e/test_homeassistant_e2e.py --admin-url http://127.0.0.1:18884 --server-url http://127.0.0.1:9098 --db-path /tmp/michi_ha_test/michi.db
"""

import argparse
import json
import sqlite3
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

def poll_until(fn, timeout=10.0, interval=0.1, desc="condition"):
    deadline = time.time() + timeout
    last_err = None
    while time.time() < deadline:
        try:
            res = fn()
            if res:
                return res
        except Exception as e:
            last_err = e
        time.sleep(interval)
    if last_err:
        raise AssertionError(f"Timed out waiting for {desc}: {last_err}")
    raise AssertionError(f"Timed out waiting for {desc}")

def get_latest_playback_session_db(db_path):
    conn = sqlite3.connect(db_path)
    cur = conn.cursor()
    cur.execute("SELECT current_track_id, current_index, playing, position_ms, volume FROM playback_sessions ORDER BY updated_at DESC LIMIT 1")
    row = cur.fetchone()
    conn.close()
    if not row:
        return None
    return {
        "current_track_id": row[0],
        "current_index": row[1],
        "playing": bool(row[2]),
        "position_ms": row[3],
        "volume": row[4],
    }

def wait_for_db_session(db_path, predicate, timeout=5.0, desc="db playback session condition"):
    def check():
        sess = get_latest_playback_session_db(db_path)
        if sess and predicate(sess):
            return sess
        return None
    return poll_until(check, timeout=timeout, desc=desc)

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
    parser.add_argument("--server-url", default="http://127.0.0.1:9098")
    parser.add_argument("--db-path", default="/tmp/michi_ha_test/michi.db")
    parser.add_argument("--username", default="admin")
    parser.add_argument("--password", default="TestAdminPassword123!")
    args = parser.parse_args()

    admin_url = args.admin_url.rstrip("/")
    server_url = args.server_url.rstrip("/")
    db_path = args.db_path

    print("=" * 60)
    print("Home Assistant & MQTT Real E2E Integration Test")
    print(f"Admin API: {admin_url} | Micro Server: {server_url} | DB: {db_path}")
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
        def check_disc():
            res = http_get(f"{admin_url}/api/mqtt/messages")
            topics = [m["topic"] for m in res.get("messages", [])]
            return any("homeassistant/sensor/michi_track_title/config" in t for t in topics)

        poll_until(check_disc, timeout=10.0, desc="MQTT discovery messages")

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
        assert msgs["michi/playback_status/state"] in (
            "idle", "preparing", "buffering", "audio_flowing", "playing", "paused", "stopped", "ended", "failed"
        )

        assert "michi/volume/state" in msgs, "missing michi/volume/state"
    test("Periodic State Publication (server_status, playback_status, volume)", test_state_publication)

    # 3. Test Command Processing (play_pause without output fails-closed to paused)
    def test_command_play_pause():
        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/play_pause/cmd",
            "payload": ""
        })
        time.sleep(0.3)
        state = http_get(f"{server_url}/api/v1/playback/state")
        assert "playing" in state, f"expected playing in state: {state}"
        assert state["playing"] is False, f"expected playing to be false without output, got {state['playing']}"
    test("Incoming MQTT Command Handling (michi/play_pause/cmd fail-closed)", test_command_play_pause)

    # 4. Test Volume Set via MQTT (non-vacuous change from initial)
    def test_command_volume_set():
        current_state = http_get(f"{server_url}/api/v1/playback/state")
        initial_vol = current_state.get("volume", 80)
        target_vol = 75 if initial_vol != 75 else 65

        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/volume_set/cmd",
            "payload": str(target_vol)
        })
        
        def check_vol():
            st = http_get(f"{server_url}/api/v1/playback/state")
            return st.get("volume") == target_vol

        poll_until(check_vol, timeout=5.0, desc=f"volume set to {target_vol}")
    test("Incoming MQTT Volume Set (michi/volume_set/cmd)", test_command_volume_set)

    # 5. Test Pause and Stop via MQTT with SQLite verification
    def test_command_pause_stop():
        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/pause/cmd",
            "payload": ""
        })
        
        def check_pause():
            st = http_get(f"{server_url}/api/v1/playback/state")
            return st.get("playing") is False
        poll_until(check_pause, timeout=3.0, desc="playback paused")

        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/stop/cmd",
            "payload": ""
        })
        
        def check_stop():
            st = http_get(f"{server_url}/api/v1/playback/state")
            return st.get("playing") is False and st.get("position_ms") == 0
        poll_until(check_stop, timeout=3.0, desc="playback stopped")

        # Verify DB directly with retry helper
        sess = wait_for_db_session(
            db_path,
            lambda s: s["playing"] is False and s["position_ms"] == 0,
            timeout=5.0,
            desc="persisted session reflecting playing=false and position_ms=0"
        )
        assert sess["playing"] is False, f"persisted session must reflect playing=false, got {sess}"
        assert sess["position_ms"] == 0, f"persisted session must reflect position_ms=0, got {sess}"
    test("Incoming MQTT Pause & Stop Commands with Direct SQLite Verification", test_command_pause_stop)

    # 6. Library Scan and Queue Navigation (Next / Previous) with direct SQLite validation
    def test_command_next_previous():
        # Trigger scan and handle response
        scan_resp = http_post(f"{server_url}/api/v1/library/scan")
        assert "scan_id" in scan_resp or "status" in scan_resp, f"unexpected scan response: {scan_resp}"

        def check_tracks():
            resp = http_get(f"{server_url}/api/v1/tracks")
            tr = resp.get("tracks", [])
            return tr if len(tr) >= 3 else None

        tracks = poll_until(check_tracks, timeout=10.0, desc="library scanner discovering >= 3 audio tracks")
        assert len(tracks) >= 3, f"expected at least 3 tracks, found {len(tracks)}"

        track_ids = [t["id"] for t in tracks[:3]]
        
        # Add tracks to queue
        http_post(f"{server_url}/api/v1/queue/items", {"track_ids": track_ids})

        # Jump to index 0 (Track A) and verify initial precondition
        http_post(f"{server_url}/api/v1/queue/jump", {"index": 0})

        def check_precondition_a():
            st = http_get(f"{server_url}/api/v1/playback/state")
            return st.get("track_id") == track_ids[0]

        poll_until(check_precondition_a, timeout=3.0, desc="precondition: track A active at index 0")

        # Issue Next Track via MQTT
        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/next_track/cmd",
            "payload": ""
        })

        def check_next():
            st = http_get(f"{server_url}/api/v1/playback/state")
            return st.get("track_id") == track_ids[1]

        poll_until(check_next, timeout=5.0, desc="Engine switching to track B on MQTT Next command")

        # Direct SQLite verification: current_track_id == B, current_index == 1
        sess_b = wait_for_db_session(
            db_path,
            lambda s: s["current_track_id"] == track_ids[1] and s["current_index"] == 1,
            timeout=5.0,
            desc="persisted session reflecting track B at index 1"
        )
        assert sess_b["current_track_id"] == track_ids[1], f"expected SQLite track_id {track_ids[1]}, got {sess_b['current_track_id']}"
        assert sess_b["current_index"] == 1, f"expected SQLite current_index 1, got {sess_b['current_index']}"

        # Issue Previous Track via MQTT
        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/previous_track/cmd",
            "payload": ""
        })

        def check_prev():
            st = http_get(f"{server_url}/api/v1/playback/state")
            return st.get("track_id") == track_ids[0]

        poll_until(check_prev, timeout=5.0, desc="Engine switching back to track A on MQTT Previous command")

        # Direct SQLite verification: current_track_id == A, current_index == 0
        sess_a = wait_for_db_session(
            db_path,
            lambda s: s["current_track_id"] == track_ids[0] and s["current_index"] == 0,
            timeout=5.0,
            desc="persisted session reflecting track A at index 0"
        )
        assert sess_a["current_track_id"] == track_ids[0], f"expected SQLite track_id {track_ids[0]}, got {sess_a['current_track_id']}"
        assert sess_a["current_index"] == 0, f"expected SQLite current_index 0, got {sess_a['current_index']}"

    test("Library Scan & MQTT Next/Previous Navigation with Direct SQLite Verification", test_command_next_previous)

    # 7. Broker Disconnect, Auto-Reconnect & Post-Reconnect Command Execution
    def test_broker_disconnect_reconnect():
        # Drop all broker connections
        http_post(f"{admin_url}/api/mqtt/drop")
        time.sleep(1.0)

        # Clear message history
        http_post(f"{admin_url}/api/mqtt/clear")

        # Restore broker
        http_post(f"{admin_url}/api/mqtt/restore")

        # Wait for reconnect by checking state publication
        def check_reconnected():
            res = http_get(f"{admin_url}/api/mqtt/messages")
            return len(res.get("messages", [])) > 0

        poll_until(check_reconnected, timeout=10.0, desc="reconnecting to MQTT broker after network recovery")

        # Now send a command after reconnect to certify that command subscription survived and works!
        http_post(f"{admin_url}/api/mqtt/publish", {
            "topic": "michi/volume_set/cmd",
            "payload": "67"
        })

        def check_vol_after_reconnect():
            st = http_get(f"{server_url}/api/v1/playback/state")
            return st.get("volume") == 67

        poll_until(check_vol_after_reconnect, timeout=5.0, desc="executing command after MQTT broker reconnect")

    test("Broker Disconnect, Auto-Reconnect & Post-Reconnect Command Execution", test_broker_disconnect_reconnect)

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

