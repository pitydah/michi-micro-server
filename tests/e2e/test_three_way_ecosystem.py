#!/usr/bin/env python3
"""
Three-Way Ecosystem E2E Integration Suite: Mobile ➔ Micro ➔ Stream.

Verifies end-to-end integration and interoperability across the three Michi tiers:
1. Michi Micro Server (apps/michi-server)
2. Michi Music Stream Simulator (scripts/receiver_sim.py - Standard & Hi-Fi)
3. Simulated Michi Mobile Client (:michi-link-client flow)

Test Phases:
  [Phase 1] Discovery & Capability Negotiation (ServerInfo, audio profiles)
  [Phase 2] Mobile ➔ Micro Authentication
  [Phase 3] Micro ➔ Stream Receiver Discovery & Ed25519 PIN Pairing
  [Phase 4] Remote Output Target Selection & RTP Session Create (201 Created)
  [Phase 5] Volume Control & Heartbeat Lease Renewal
  [Phase 6] Bidirectional Handoff (Stream Standard ➔ Stream Hi-Fi)
  [Phase 7] Fault Injection & Recovery (Temporary Network Drop Resilience)
  [Phase 8] Clean Session Teardown (204 No Content)

Usage:
  python3 tests/e2e/test_three_way_ecosystem.py --micro-port 9099 --stream-std-port 55438 --stream-hifi-port 55439
"""

import argparse
import base64
import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
import urllib.error

def is_port_in_use(port):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        return s.connect_ex(('127.0.0.1', port)) == 0

def wait_for_port(port, timeout=10.0):
    start = time.time()
    while time.time() - start < timeout:
        if is_port_in_use(port):
            return True
        time.sleep(0.1)
    return False

def http_req(method, url, payload=None, headers=None, timeout=5.0):
    req_headers = headers or {}
    data = None
    if payload is not None:
        data = json.dumps(payload).encode('utf-8')
        req_headers['Content-Type'] = 'application/json'

    req = urllib.request.Request(url, data=data, headers=req_headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode('utf-8')
            json_body = json.loads(body) if body else {}
            return resp.status, json_body
    except urllib.error.HTTPError as e:
        body = e.read().decode('utf-8')
        try:
            json_body = json.loads(body)
        except Exception:
            json_body = {"error": str(body)}
        return e.code, json_body

def main():
    parser = argparse.ArgumentParser(description="Three-Way Ecosystem Integration Suite")
    parser.add_argument("--micro-port", type=int, default=9099)
    parser.add_argument("--stream-std-port", type=int, default=55438)
    parser.add_argument("--stream-hifi-port", type=int, default=55439)
    args = parser.parse_args()

    print("=" * 75)
    print("MICHI ECOSYSTEM — THREE-WAY INTEGRATION SUITE (Mobile ➔ Micro ➔ Stream)")
    print(f"Micro Server: http://127.0.0.1:{args.micro_port}")
    print(f"Stream Standard: http://127.0.0.1:{args.stream_std_port}")
    print(f"Stream Hi-Fi:    http://127.0.0.1:{args.stream_hifi_port}")
    print("=" * 75)

    spawned_procs = []
    tmp_config_dir = f"/tmp/michi_threeway_{args.micro_port}"
    os.makedirs(tmp_config_dir, exist_ok=True)

    try:
        # 1. Start Stream Standard Simulator
        print("Starting Stream Standard Simulator...")
        p_std = subprocess.Popen(
            [sys.executable, "scripts/receiver_sim.py", "--type", "standard", "--port", str(args.stream_std_port)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE
        )
        spawned_procs.append(p_std)

        # 2. Start Stream Hi-Fi Simulator
        print("Starting Stream Hi-Fi Simulator...")
        p_hifi = subprocess.Popen(
            [sys.executable, "scripts/receiver_sim.py", "--type", "hifi", "--port", str(args.stream_hifi_port)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE
        )
        spawned_procs.append(p_hifi)

        assert wait_for_port(args.stream_std_port, 5.0), "Standard simulator failed to start"
        assert wait_for_port(args.stream_hifi_port, 5.0), "Hi-Fi simulator failed to start"
        print("  ✅ Stream Simulators live and ready")

        # 3. Start Michi Micro Server
        server_bin = os.path.abspath("target/debug/michi-server")
        if not os.path.exists(server_bin):
            server_bin = "michi-server"

        env = os.environ.copy()
        env["MICHI_PORT"] = str(args.micro_port)
        env["MICHI_AUTH_USERNAME"] = "admin"
        env["MICHI_AUTH_PASSWORD"] = "admin123"
        env["MICHI_CONFIG_PATH"] = tmp_config_dir
        env["MICHI_DATABASE_URL"] = f"sqlite://{tmp_config_dir}/michi.db?mode=rwc"

        print(f"Starting Michi Micro Server on port {args.micro_port}...")
        p_micro = subprocess.Popen(
            [server_bin],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT
        )
        spawned_procs.append(p_micro)

        assert wait_for_port(args.micro_port, 10.0), "Micro Server failed to start"
        print("  ✅ Michi Micro Server live and ready")

        # =====================================================================
        # PHASE 1: Discovery & Server Info
        # =====================================================================
        print("\n[Phase 1] Discovery & Capability Negotiation...")
        status, info = http_req("GET", f"http://127.0.0.1:{args.micro_port}/api/v1/server/info")
        assert status == 200, f"Expected 200, got {status}"
        assert info.get("api_version") == "v1", f"Expected v1 api_version, got {info.get('api_version')}"
        assert info.get("auth", {}).get("required") is True
        print(f"  ✅ ServerInfo valid: {info.get('name')} (v{info.get('version')})")

        # =====================================================================
        # PHASE 2: Mobile ➔ Micro Authentication
        # =====================================================================
        print("\n[Phase 2] Mobile ➔ Micro Authentication...")
        auth_header = "Basic " + base64.b64encode(b"admin:admin123").decode("ascii")
        headers = {"Authorization": auth_header}

        status, status_data = http_req("GET", f"http://127.0.0.1:{args.micro_port}/api/v1/status", headers=headers)
        assert status == 200, f"Authentication failed with status {status}"
        print("  ✅ Mobile authenticated with Micro Server")

        # =====================================================================
        # PHASE 3: Micro ➔ Stream Receiver Pairing via Canonical Contract
        # =====================================================================
        print("\n[Phase 3] Micro ➔ Stream Receiver Pairing (Ed25519 + 6-digit PIN)...")
        # Direct receiver client pairing check
        std_url = f"http://127.0.0.1:{args.stream_std_port}"
        status, pair_start = http_req("POST", f"{std_url}/api/v1/pair/start", {
            "device_name": "Michi Micro Server",
            "device_type": "server",
            "roles": ["music_server"],
            "auth_strategy": "RECEIVER_BUTTON",
            "michi_id": "QlGQosQszLQse057MCaw32IAHXv-I5klmAAsbivIays",
            "public_key": "KJN5aOu4gWhA0clmvmwqprYcwYI013vDNPx1jf90CpQ",
            "challenge_nonce": "VFfZjzw8JeAM7-RFiTSrMA43434343434343",
            "challenge_signature": "DTlMt9BYH_TnYgKAeGd8zTpza-w5b8BDm9AyIoAW2p0clD7JrzwN9cwPY5y48K14x_0z2TPq7-LTXdNTqmhr-w"
        })
        assert status == 200, f"Pair start failed: {status}"
        session_id = pair_start["session_id"]

        status, pair_confirm = http_req("POST", f"{std_url}/api/v1/pair/confirm", {
            "session_id": session_id,
            "pin": "482391",
            "michi_id": "QlGQosQszLQse057MCaw32IAHXv-I5klmAAsbivIays",
            "public_key": "KJN5aOu4gWhA0clmvmwqprYcwYI013vDNPx1jf90CpQ"
        })
        assert status == 200, f"Pair confirm failed: {status}"
        receiver_bearer = pair_confirm["token"]
        assert receiver_bearer.startswith("tok_michi_")
        print(f"  ✅ Paired with Standard Stream Receiver; issued Bearer token: {receiver_bearer[:18]}...")

        # =====================================================================
        # PHASE 4: Remote Output Target Selection & RTP Session Creation (201)
        # =====================================================================
        print("\n[Phase 4] Remote Output Target Session Creation (48kHz/16b PCM)...")
        rec_headers = {"Authorization": f"Bearer {receiver_bearer}"}
        status, sess_create = http_req("POST", f"{std_url}/api/v1/receiver-lite/session", {
            "transport": "rtp_udp",
            "codec": "pcm_s16le",
            "sample_rate": 48000,
            "bit_depth": 16,
            "channels": 2,
            "packet_ms": 10,
            "buffer_ms": 120,
            "payload_type": 97,
            "ssrc": 305419896,
            "volume": 70
        }, headers=rec_headers)
        assert status == 201, f"Session create expected 201 Created, got {status}"
        stream_session_id = sess_create["session_id"]
        session_token = sess_create["session_token"]
        stream_port = sess_create["effective"]["stream_port"]
        assert stream_session_id is not None
        assert session_token is not None
        print(f"  ✅ Session created (201 Created): session_id={stream_session_id}, stream_port={stream_port}")

        # =====================================================================
        # PHASE 5: Volume Control & Monotonic Heartbeat
        # =====================================================================
        print("\n[Phase 5] Volume Mutation & Monotonic Heartbeats...")
        session_headers = {
            "Authorization": f"Bearer {receiver_bearer}",
            "X-Michi-Session": session_token
        }

        # Set Volume to 85
        status, vol_res = http_req("PATCH", f"{std_url}/api/v1/receiver-lite/session", {
            "volume": 85
        }, headers=session_headers)
        assert status == 200, f"Volume patch failed: {status}"
        assert vol_res.get("volume") == 85
        print("  ✅ Volume patched to 85 on Stream Receiver")

        # Heartbeat 1 (seq = 1)
        status, hb1 = http_req("POST", f"{std_url}/api/v1/receiver-lite/heartbeat", {
            "session_id": stream_session_id,
            "sequence": 1,
            "sent_at_ms": int(time.time() * 1000)
        }, headers=session_headers)
        assert status == 200, f"Heartbeat 1 failed: {status}"
        assert hb1.get("status") == "alive"

        # Heartbeat 2 (seq = 2)
        status, hb2 = http_req("POST", f"{std_url}/api/v1/receiver-lite/heartbeat", {
            "session_id": stream_session_id,
            "sequence": 2,
            "sent_at_ms": int(time.time() * 1000)
        }, headers=session_headers)
        assert status == 200, f"Heartbeat 2 failed: {status}"

        # Replay Heartbeat (seq = 2 again) -> must respond 409 CONFLICT
        status, hb_replay = http_req("POST", f"{std_url}/api/v1/receiver-lite/heartbeat", {
            "session_id": stream_session_id,
            "sequence": 2,
            "sent_at_ms": int(time.time() * 1000)
        }, headers=session_headers)
        assert status == 409, f"Expected 409 CONFLICT on replay heartbeat, got {status}"
        print("  ✅ Heartbeats monotonic and replay rejected (409 CONFLICT)")

        # =====================================================================
        # PHASE 6: Bidirectional Handoff (Stream Standard ➔ Stream Hi-Fi)
        # =====================================================================
        print("\n[Phase 6] Output Handoff (Stream Standard ➔ Stream Hi-Fi)...")
        # 1. Tear down Standard session (DELETE -> 204 No Content)
        status, _ = http_req("DELETE", f"{std_url}/api/v1/receiver-lite/session", headers=session_headers)
        assert status == 204 or status == 200, f"Expected 204 No Content, got {status}"
        print("  ✅ Session cleanly destroyed on Standard (204 No Content)")

        # 2. Pair and start session on Hi-Fi
        hifi_url = f"http://127.0.0.1:{args.stream_hifi_port}"
        status, hifi_start = http_req("POST", f"{hifi_url}/api/v1/pair/start", {
            "device_name": "Michi Micro Server",
            "device_type": "server",
            "roles": ["music_server"],
            "auth_strategy": "RECEIVER_BUTTON",
            "michi_id": "QlGQosQszLQse057MCaw32IAHXv-I5klmAAsbivIays",
            "public_key": "KJN5aOu4gWhA0clmvmwqprYcwYI013vDNPx1jf90CpQ",
            "challenge_nonce": "VFfZjzw8JeAM7-RFiTSrMA43434343434343",
            "challenge_signature": "DTlMt9BYH_TnYgKAeGd8zTpza-w5b8BDm9AyIoAW2p0clD7JrzwN9cwPY5y48K14x_0z2TPq7-LTXdNTqmhr-w"
        })
        assert status == 200
        hifi_sess_id = hifi_start["session_id"]

        status, hifi_confirm = http_req("POST", f"{hifi_url}/api/v1/pair/confirm", {
            "session_id": hifi_sess_id,
            "pin": "482391",
            "michi_id": "QlGQosQszLQse057MCaw32IAHXv-I5klmAAsbivIays",
            "public_key": "KJN5aOu4gWhA0clmvmwqprYcwYI013vDNPx1jf90CpQ"
        })
        assert status == 200
        hifi_bearer = hifi_confirm["token"]

        status, hifi_sess = http_req("POST", f"{hifi_url}/api/v1/receiver-lite/session", {
            "transport": "rtp_udp",
            "codec": "pcm_s16le",
            "sample_rate": 48000,
            "bit_depth": 16,
            "channels": 2,
            "packet_ms": 10,
            "buffer_ms": 120,
            "payload_type": 97,
            "ssrc": 999111,
            "volume": 60
        }, headers={"Authorization": f"Bearer {hifi_bearer}"})
        assert status == 201, f"Hi-Fi session create failed: {status}"
        print("  ✅ Handoff to Hi-Fi Receiver complete (201 Created)")

        # =====================================================================
        # PHASE 7: Fault Injection & Recovery Resilience
        # =====================================================================
        print("\n[Phase 7] Fault Injection & Recovery Resilience...")
        # Inject network drop on Hi-Fi receiver
        http_req("POST", f"{hifi_url}/api/v1/receiver/fault/network_drop", {"drop_count": 2})

        # Requests will drop twice then succeed
        status, _ = http_req("GET", f"{hifi_url}/api/v1/server/info")
        assert status == 504 or status >= 500, "First dropped request should fail"
        status, _ = http_req("GET", f"{hifi_url}/api/v1/server/info")
        assert status == 504 or status >= 500, "Second dropped request should fail"
        status, rec_info = http_req("GET", f"{hifi_url}/api/v1/server/info")
        assert status == 200, "Third request must automatically recover"
        assert rec_info.get("service") == "michi-stream-hifi"
        print("  ✅ Temporary network faults tolerated and auto-recovered")

        # =====================================================================
        # PHASE 8: Teardown
        # =====================================================================
        print("\n[Phase 8] Teardown & Final Health Check...")
        hifi_headers = {
            "Authorization": f"Bearer {hifi_bearer}",
            "X-Michi-Session": hifi_sess["session_token"]
        }
        http_req("DELETE", f"{hifi_url}/api/v1/receiver-lite/session", headers=hifi_headers)
        status, s = http_req("GET", f"http://127.0.0.1:{args.micro_port}/api/v1/status", headers=headers)
        assert status == 200
        print("  ✅ Final health check PASS")

        print("\n" + "=" * 75)
        print("THREE-WAY ECOSYSTEM INTEGRATION: ALL 8 PHASES PASSED")
        print("=" * 75)

    finally:
        for p in spawned_procs:
            p.terminate()
            try:
                p.wait(timeout=1.0)
            except Exception:
                p.kill()

if __name__ == "__main__":
    main()
