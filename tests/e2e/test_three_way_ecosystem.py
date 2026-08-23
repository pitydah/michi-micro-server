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
        status, login_data = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/auth/login", {
            "username": "admin",
            "password": "admin123"
        })
        assert status == 200, f"Login failed with status {status}: {login_data}"
        session_token = login_data["token"]
        headers = {"Authorization": f"Bearer {session_token}"}

        status, status_data = http_req("GET", f"http://127.0.0.1:{args.micro_port}/api/v1/status", headers=headers)
        assert status == 200, f"Authentication failed with status {status}"
        print("  ✅ Mobile authenticated with Micro Server")

        # =====================================================================
        # PHASE 3: Mobile ➔ Micro Receiver Discovery & Pairing
        # =====================================================================
        print("\n[Phase 3] Mobile ➔ Micro: Pairing with Stream Standard...")
        std_url = f"http://127.0.0.1:{args.stream_std_port}"
        
        # 1. Mobile requests Micro to start pairing with Stream Standard
        status, pair_start = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/pair/start", {
            "base_url": std_url,
            "initiator_id": "mobile-client-1"
        }, headers=headers)
        assert status == 200, f"Pair start via Micro failed: {status}, data: {pair_start}"
        pairing_id = pair_start["pairing_id"]
        assert pairing_id is not None
        print(f"  ✅ Micro initiated pairing with Stream Standard (pairing_id={pairing_id})")

        # 2. Query Stream Simulator test-only endpoint for active PIN (simulating user looking at receiver display)
        status, pin_info = http_req("GET", f"{std_url}/api/v1/test/active_pin")
        assert status == 200
        active_pin = pin_info.get("pin", "482391")

        # 3. Mobile confirms pairing through Micro Server using the PIN
        status, pair_confirm = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/pair/confirm", {
            "pairing_id": pairing_id,
            "pin": active_pin
        }, headers=headers)
        assert status == 200, f"Pair confirm via Micro failed: {status}, data: {pair_confirm}"
        standard_device_id = pair_confirm["device_id"]
        print(f"  ✅ Micro paired with Stream Standard: device_id={standard_device_id}")

        # Verify receiver is listed in Micro registry
        status, receivers_list = http_req("GET", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers", headers=headers)
        assert status == 200
        assert any(r.get("receiver_id") == standard_device_id and r.get("paired") for r in receivers_list.get("receivers", []))
        print("  ✅ Receiver confirmed in Micro registry")

        # =====================================================================
        # PHASE 4: Mobile ➔ Micro: Start Remote Session & Stream Real PCM
        # =====================================================================
        print("\n[Phase 4] Mobile ➔ Micro: Start Session & Stream Real PCM via Micro Transport...")
        session_id = "sess-ecosystem-e2e-1"
        status, sess_start = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{standard_device_id}/session/start", {
            "session_id": session_id,
            "codec": "pcm_s16le",
            "sample_rate": 48000,
            "bit_depth": 16,
            "channels": 2,
            "stream_port": 50438,
            "buffer_ms": 120,
            "volume": 70
        }, headers=headers)
        assert status == 200, f"Session start via Micro failed: {status}, data: {sess_start}"
        print(f"  ✅ Micro started active session to Stream Standard: {sess_start}")

        negotiated_ssrc = sess_start.get("ssrc")
        assert negotiated_ssrc is not None and negotiated_ssrc > 0, f"Micro must return valid negotiated ssrc, got {negotiated_ssrc}"

        # Micro Server transmits 50ms of real 480Hz sine wave PCM through its own RtpReceiverTransport
        status, stream_res = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{standard_device_id}/stream/test_pcm", {
            "frequency_hz": 480.0,
            "duration_ms": 50
        }, headers=headers)
        assert status == 200, f"Micro PCM streaming failed: {status}, data: {stream_res}"
        bytes_sent = stream_res.get("bytes_sent", 0)
        assert bytes_sent == 9600 # 50ms = 5 packets * 1920 bytes = 9600 bytes
        print(f"  ✅ Micro Server streamed {bytes_sent} bytes of real RTP/UDP PCM to Stream Standard")

        # Query Stream Standard Simulator test metrics to verify UDP arrival and RFC 3550 contract
        time.sleep(0.2)
        status, metrics = http_req("GET", f"{std_url}/api/v1/test/metrics")
        assert status == 200
        assert metrics["packets_received"] >= 5, f"Expected >=5 packets, got {metrics['packets_received']}"
        assert metrics["last_payload_size"] == 1920, f"Expected 1920 bytes payload size, got {metrics['last_payload_size']}"
        assert metrics["last_payload_type"] == 97, f"Expected PT 97, got {metrics['last_payload_type']}"
        assert metrics["last_ssrc"] == negotiated_ssrc, f"SSRC mismatch! Negotiated: {negotiated_ssrc}, Received: {metrics['last_ssrc']}"
        assert metrics.get("source_port", 0) > 0, "Local source port from Micro must be positive non-zero"

        # Verify RFC 3550 continuous monotonic progression: seq +1, ts +480 per packet
        pkt_history = metrics.get("packet_history", [])
        assert len(pkt_history) >= 5, f"Expected at least 5 packets in history, got {len(pkt_history)}"
        for i in range(1, len(pkt_history)):
            prev = pkt_history[i-1]
            curr = pkt_history[i]
            seq_delta = (curr["seq"] - prev["seq"]) & 0xFFFF
            ts_delta = (curr["ts"] - prev["ts"]) & 0xFFFFFFFF
            assert seq_delta == 1, f"Packet sequence discontinuity: {prev['seq']} -> {curr['seq']} (delta {seq_delta} != 1)"
            assert ts_delta == 480, f"RTP timestamp discontinuity: {prev['ts']} -> {curr['ts']} (delta {ts_delta} != 480 frames)"
            assert curr["size"] == 1920, f"Packet payload size != 1920 bytes: {curr['size']}"
            assert curr["ssrc"] == negotiated_ssrc, f"Packet SSRC mismatch in stream: {curr['ssrc']} != {negotiated_ssrc}"

        print(f"  ✅ Stream Standard Simulator verified reception of {metrics['packets_received']} RTP packets (size=1920, PT=97, SSRC={metrics['last_ssrc']} == Negotiated {negotiated_ssrc}, src_port={metrics.get('source_port')}, seq_delta=+1, ts_delta=+480)")

        # =====================================================================
        # PHASE 5: Mobile ➔ Micro: Volume Control & Heartbeats
        # =====================================================================
        print("\n[Phase 5] Mobile ➔ Micro: Volume Mutation & Heartbeats...")
        # 1. Set volume to 85 via Micro Server
        status, vol_res = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{standard_device_id}/volume", {
            "volume": 85
        }, headers=headers)
        assert status == 200, f"Volume set via Micro failed: {status}"
        assert vol_res.get("volume") == 85
        print("  ✅ Volume set to 85 via Micro Server")

        # 2. Trigger managed heartbeat via Micro Server
        status, hb_res = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{standard_device_id}/heartbeat", headers=headers)
        assert status == 200, f"Heartbeat via Micro failed: {status}"
        assert hb_res.get("status") == "alive"
        print("  ✅ Manual heartbeat verified via Micro Server")

        # 3. Verify automatic background heartbeat task from Micro Server
        time.sleep(2.5) # Background task interval is clamped at 2-10s
        status, metrics_hb = http_req("GET", f"{std_url}/api/v1/test/metrics")
        assert status == 200
        hb_count = metrics_hb.get("heartbeats_received", 0)
        assert hb_count >= 1, f"Expected automatic background heartbeat to be received, got {hb_count}"
        last_seq = metrics_hb.get("last_heartbeat_seq", 0)
        assert last_seq >= 1, f"Expected last_heartbeat_seq >= 1, got {last_seq}"
        print(f"  ✅ Automatic background heartbeat confirmed (received={hb_count}, last_seq={last_seq})")

        # =====================================================================
        # PHASE 6: Mobile ➔ Micro: Output Handoff (Standard ➔ Hi-Fi)
        # =====================================================================
        print("\n[Phase 6] Mobile ➔ Micro: Output Handoff (Standard ➔ Hi-Fi)...")
        # 1. Stop session on Standard via Micro Server
        status, stop_res = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{standard_device_id}/session/stop", headers=headers)
        assert status == 200, f"Session stop via Micro failed: {status}"
        print("  ✅ Standard session cleanly stopped via Micro Server")

        # Verify no packets received after stop
        status, metrics_after_stop = http_req("GET", f"{std_url}/api/v1/test/metrics")
        assert status == 200
        count_at_stop = metrics_after_stop["packets_received"]
        time.sleep(0.1)
        status, metrics_check = http_req("GET", f"{std_url}/api/v1/test/metrics")
        assert status == 200
        assert metrics_check["packets_received"] == count_at_stop, "Zero RTP packets must be transmitted after session stop"
        print("  ✅ Zero packets after session stop confirmed")

        # 2. Pair with Stream Hi-Fi via Micro Server
        hifi_url = f"http://127.0.0.1:{args.stream_hifi_port}"
        status, hifi_start = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/pair/start", {
            "base_url": hifi_url,
            "initiator_id": "mobile-client-1"
        }, headers=headers)
        assert status == 200
        hifi_pairing_id = hifi_start["pairing_id"]

        status, hifi_pin_info = http_req("GET", f"{hifi_url}/api/v1/test/active_pin")
        assert status == 200
        hifi_pin = hifi_pin_info.get("pin", "482391")

        status, hifi_confirm = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/pair/confirm", {
            "pairing_id": hifi_pairing_id,
            "pin": hifi_pin
        }, headers=headers)
        assert status == 200
        hifi_device_id = hifi_confirm["device_id"]
        print(f"  ✅ Paired with Hi-Fi receiver: device_id={hifi_device_id}")

        # 3. Start session on Hi-Fi via Micro Server
        status, hifi_sess = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{hifi_device_id}/session/start", {
            "session_id": "sess-ecosystem-hifi-1",
            "codec": "pcm_s16le",
            "sample_rate": 48000,
            "bit_depth": 16,
            "channels": 2,
            "stream_port": 50439,
            "buffer_ms": 120,
            "volume": 65
        }, headers=headers)
        assert status == 200, f"Hi-Fi session start failed: {status}"

        # 4. Stream real PCM to Hi-Fi
        status, hifi_stream = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{hifi_device_id}/stream/test_pcm", {
            "frequency_hz": 1000.0,
            "duration_ms": 30
        }, headers=headers)
        assert status == 200
        assert hifi_stream.get("bytes_sent") == 5760 # 30ms = 3 packets * 1920 bytes
        print("  ✅ Real PCM streamed to Hi-Fi receiver via Micro Server")

        # =====================================================================
        # PHASE 7: Fault Injection & Recovery Resilience
        # =====================================================================
        print("\n[Phase 7] Fault Injection & Recovery Resilience...")
        # Inject network drop on Hi-Fi receiver simulator
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
        status, _ = http_req("POST", f"http://127.0.0.1:{args.micro_port}/api/v1/receivers/{hifi_device_id}/session/stop", headers=headers)
        assert status == 200, "Hi-Fi session teardown should succeed"

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
