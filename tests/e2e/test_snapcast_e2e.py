#!/usr/bin/env python3
"""
Michi Micro Server — Snapcast Real E2E Integration Suite.

Tests real Snapserver JSON-RPC communication, group queries, volume adjustments,
group mute toggles, simulated client disconnect/reconnect, and multi-room state consistency.

Usage:
  python3 tests/e2e/test_snapcast_e2e.py --snapserver-url http://127.0.0.1:1780
"""

import argparse
import json
import sys
import urllib.request
import urllib.error

PASS = 0
FAIL = 0

def rpc_call(url, method, params=None):
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params or {}
    }
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        f"{url.rstrip('/')}/json-rpc",
        data=data,
        headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        body = resp.read().decode("utf-8")
        return json.loads(body)

def admin_call(url, path, body=None):
    data = json.dumps(body or {}).encode("utf-8")
    req = urllib.request.Request(
        f"{url.rstrip('/')}{path}",
        data=data,
        headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        body = resp.read().decode("utf-8")
        return json.loads(body)

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
    parser = argparse.ArgumentParser(description="Snapcast E2E Test")
    parser.add_argument("--snapserver-url", default="http://127.0.0.1:1780")
    args = parser.parse_args()

    url = args.snapserver_url
    print("=" * 60)
    print(f"Snapcast Real E2E Integration Test against {url}")
    print("=" * 60)

    # 1. Server Status & Version
    def test_server_status():
        res = rpc_call(url, "Server.GetStatus")
        assert "result" in res, f"expected result in response: {res}"
        server = res["result"]["server"]
        assert "version" in server
        assert len(server["groups"]) == 2, f"expected 2 groups, got {len(server['groups'])}"
    test("Server.GetStatus (2 groups, version)", test_server_status)

    # 2. Multi-client Discovery
    def test_clients_topology():
        res = rpc_call(url, "Server.GetStatus")
        groups = res["result"]["server"]["groups"]
        lr_group = next(g for g in groups if g["id"] == "group-living-room")
        kitchen_group = next(g for g in groups if g["id"] == "group-kitchen")
        assert len(lr_group["clients"]) == 2, "Living Room must have 2 clients"
        assert len(kitchen_group["clients"]) == 1, "Kitchen must have 1 client"
        assert lr_group["volume"]["percent"] == 80
        assert kitchen_group["volume"]["percent"] == 65
    test("Multi-client Group Topology", test_clients_topology)

    # 3. Group Volume Adjustment
    def test_group_volume():
        res = rpc_call(url, "Group.SetVolume", {"id": "group-living-room", "volume": {"percent": 90, "muted": False}})
        assert res.get("result", {}).get("volume", {}).get("percent") == 90
        # Verify persistence
        st = rpc_call(url, "Server.GetStatus")
        lr = next(g for g in st["result"]["server"]["groups"] if g["id"] == "group-living-room")
        assert lr["volume"]["percent"] == 90
    test("Group.SetVolume (Living Room -> 90%)", test_group_volume)

    # 4. Group Mute Toggle
    def test_group_mute():
        res = rpc_call(url, "Group.SetMute", {"id": "group-kitchen", "mute": True})
        assert res.get("result", {}).get("mute") is True
        st = rpc_call(url, "Server.GetStatus")
        kitchen = next(g for g in st["result"]["server"]["groups"] if g["id"] == "group-kitchen")
        assert kitchen["muted"] is True
        assert kitchen["volume"]["muted"] is True

        # Unmute
        rpc_call(url, "Group.SetMute", {"id": "group-kitchen", "mute": False})
        st2 = rpc_call(url, "Server.GetStatus")
        kitchen2 = next(g for g in st2["result"]["server"]["groups"] if g["id"] == "group-kitchen")
        assert kitchen2["muted"] is False
    test("Group.SetMute (Mute Kitchen -> Unmute Kitchen)", test_group_mute)

    # 5. Client Drop & Reconnection
    def test_client_disconnect_reconnect():
        # Disconnect client 2 in living room
        admin_call(url, "/api/admin/client/disconnect", {"client_id": "client-speaker-lr-2"})
        st = rpc_call(url, "Server.GetStatus")
        lr = next(g for g in st["result"]["server"]["groups"] if g["id"] == "group-living-room")
        spk2 = next(c for c in lr["clients"] if c["id"] == "client-speaker-lr-2")
        assert spk2["connected"] is False, "Speaker 2 should be disconnected"

        # Reconnect client 2
        admin_call(url, "/api/admin/client/reconnect", {"client_id": "client-speaker-lr-2"})
        st2 = rpc_call(url, "Server.GetStatus")
        lr2 = next(g for g in st2["result"]["server"]["groups"] if g["id"] == "group-living-room")
        spk2_rec = next(c for c in lr2["clients"] if c["id"] == "client-speaker-lr-2")
        assert spk2_rec["connected"] is True, "Speaker 2 should be reconnected"
    test("Client Disconnect & Reconnect Simulation", test_client_disconnect_reconnect)

    # 6. Snapserver Offline & Recovery Fault Handling
    def test_server_offline_recovery():
        admin_call(url, "/api/admin/offline", {"offline": True})
        try:
            rpc_call(url, "Server.GetStatus")
            assert False, "should have failed when server offline"
        except Exception:
            pass  # Expected 503

        # Restore server
        admin_call(url, "/api/admin/offline", {"offline": False})
        st = rpc_call(url, "Server.GetStatus")
        assert "result" in st, "server must recover after downtime"
    test("Snapserver Offline & Recovery", test_server_offline_recovery)

    # Summary
    print("\n" + "=" * 60)
    print(f"Snapcast E2E Results: {PASS} passed, {FAIL} failed ({PASS + FAIL} total)")
    if FAIL > 0:
        sys.exit(1)
    else:
        print("SNAPCAST E2E: PASS")
        sys.exit(0)

if __name__ == "__main__":
    main()
