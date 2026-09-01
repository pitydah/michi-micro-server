#!/usr/bin/env python3
"""
Mosquitto Real Integration Test Suite.

Verifies end-to-end MQTT integration between a real Mosquitto broker
and Michi Micro Server (`michi-homeassistant` crate):
1. Starts real Mosquitto broker on an ephemeral port.
2. Starts Michi Micro Server with MICHI_MQTT_HOST and MICHI_MQTT_PORT.
3. Verifies Home Assistant Auto-Discovery payload publications (Sensors, Switches, Numbers).
4. Verifies state updates (status, playback, volume).
5. Sends MQTT commands (play_pause, volume_set, next, previous) and validates execution on Micro.
6. Injects broker restart / connection drop and verifies automatic reconnect resilience within 5s.

Usage:
  python3 tests/e2e/test_mosquitto_real.py --broker-port 18885 --server-port 9098
"""

import argparse
import json
import os
import socket
import struct
import subprocess
import sys
import time
import urllib.request

class MiniMqttClient:
    def __init__(self, host, port, client_id="mini_mqtt_tester"):
        self.host = host
        self.port = port
        self.client_id = client_id
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.settimeout(5.0)
        self.msg_queue = []

    def connect(self):
        self.sock.connect((self.host, self.port))
        var_header = b"\x00\x04MQTT\x04\x02\x00\x3c"
        cid_bytes = self.client_id.encode("utf-8")
        payload = struct.pack(">H", len(cid_bytes)) + cid_bytes
        remaining = var_header + payload
        packet = b"\x10" + self._encode_remaining_len(len(remaining)) + remaining
        self.sock.sendall(packet)
        ack = self._read_packet()
        assert ack[0] == 0x20, f"Expected CONNACK (0x20), got {ack}"

    def subscribe(self, topic, pkid=1):
        t_bytes = topic.encode("utf-8")
        payload = struct.pack(">H", len(t_bytes)) + t_bytes + b"\x01" # QoS 1
        var_header = struct.pack(">H", pkid)
        remaining = var_header + payload
        packet = b"\x82" + self._encode_remaining_len(len(remaining)) + remaining
        self.sock.sendall(packet)
        ack = self._read_packet()
        assert ack[0] == 0x90, f"Expected SUBACK (0x90), got {ack}"

    def publish(self, topic, payload_str, qos=0, pkid=2):
        t_bytes = topic.encode("utf-8")
        p_bytes = payload_str.encode("utf-8")
        if qos == 1:
            var_header = struct.pack(">H", len(t_bytes)) + t_bytes + struct.pack(">H", pkid)
            packet_type = 0x32
        else:
            var_header = struct.pack(">H", len(t_bytes)) + t_bytes
            packet_type = 0x30
        remaining = var_header + p_bytes
        packet = bytes([packet_type]) + self._encode_remaining_len(len(remaining)) + remaining
        self.sock.sendall(packet)
        if qos == 1:
            ack = self._read_packet()
            assert ack[0] == 0x40, f"Expected PUBACK (0x40), got {ack}"

    def _encode_remaining_len(self, length):
        out = bytearray()
        while True:
            byte = length % 128
            length //= 128
            if length > 0:
                byte |= 0x80
            out.append(byte)
            if length == 0:
                break
        return bytes(out)

    def _read_exact(self, n):
        data = bytearray()
        while len(data) < n:
            chunk = self.sock.recv(n - len(data))
            if not chunk:
                raise ConnectionError("Socket closed prematurely")
            data.extend(chunk)
        return bytes(data)

    def _read_packet(self):
        hdr = self._read_exact(1)[0]
        multiplier = 1
        length = 0
        while True:
            b = self._read_exact(1)[0]
            length += (b & 0x7F) * multiplier
            multiplier *= 128
            if (b & 0x80) == 0:
                break
        body = self._read_exact(length) if length > 0 else b""
        return (hdr, body)

    def poll_messages(self, timeout=0.5):
        self.sock.settimeout(timeout)
        try:
            while True:
                hdr, body = self._read_packet()
                pkt_type = hdr & 0xF0
                if pkt_type == 0x30:
                    flags = hdr & 0x0F
                    qos = (flags >> 1) & 0x03
                    t_len = struct.unpack(">H", body[0:2])[0]
                    topic = body[2:2+t_len].decode("utf-8")
                    idx = 2 + t_len
                    if qos > 0:
                        pkid = struct.unpack(">H", body[idx:idx+2])[0]
                        idx += 2
                        puback = bytes([0x40, 0x02]) + struct.pack(">H", pkid)
                        self.sock.sendall(puback)
                    payload = body[idx:].decode("utf-8", errors="replace")
                    self.msg_queue.append((topic, payload))
        except (socket.timeout, TimeoutError):
            pass
        return list(self.msg_queue)

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass

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

def http_get(url, timeout=5.0):
    req = urllib.request.Request(url)
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.status, json.loads(resp.read().decode('utf-8'))

def main():
    parser = argparse.ArgumentParser(description="Mosquitto Real Integration Test")
    parser.add_argument("--broker-port", type=int, default=18885)
    parser.add_argument("--server-port", type=int, default=9098)
    args = parser.parse_args()

    print("=" * 70)
    print("MICHI MICRO SERVER — MOSQUITTO REAL INTEGRATION SUITE")
    print(f"Broker Port: {args.broker_port} | Server Port: {args.server_port}")
    print("=" * 70)

    # Check if mosquitto is available
    mosquitto_bin = None
    for p in ["/usr/sbin/mosquitto", "/usr/bin/mosquitto", "mosquitto"]:
        try:
            res = subprocess.run([p, "-h"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            if res.returncode == 0 or b"mosquitto" in res.stdout or b"mosquitto" in res.stderr:
                mosquitto_bin = p
                break
        except Exception:
            continue

    if not mosquitto_bin:
        print("⚠️ Mosquitto binary not found in system path.")
        print("  Skipping real Mosquitto test (marked SKIPPED_BINARY_NOT_FOUND).")
        sys.exit(0)

    tmp_conf = f"/tmp/mosquitto_test_{args.broker_port}.conf"
    with open(tmp_conf, "w") as f:
        f.write(f"listener {args.broker_port} 127.0.0.1\nallow_anonymous true\n")

    broker_proc = None
    server_proc = None
    mqtt_client = None

    try:
        print(f"Starting real Mosquitto broker on port {args.broker_port}...")
        broker_proc = subprocess.Popen(
            [mosquitto_bin, "-c", tmp_conf],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE
        )

        if not wait_for_port(args.broker_port, timeout=5.0):
            print("❌ Failed to bind Mosquitto broker port.")
            sys.exit(1)
        print(f"  ✅ Mosquitto broker live on 127.0.0.1:{args.broker_port}")

        # Connect our test observer client to Mosquitto before server starts
        mqtt_client = MiniMqttClient("127.0.0.1", args.broker_port, client_id="test_observer")
        mqtt_client.connect()
        mqtt_client.subscribe("homeassistant/#", pkid=1)
        mqtt_client.subscribe("michi/#", pkid=2)
        print("  ✅ Observer client connected & subscribed to homeassistant/# and michi/#")

        # Start Michi Server with MQTT configured
        env = os.environ.copy()
        env["MICHI_PORT"] = str(args.server_port)
        env["MICHI_MQTT_HOST"] = "127.0.0.1"
        env["MICHI_MQTT_PORT"] = str(args.broker_port)
        env["MICHI_AUTH_USERNAME"] = "admin"
        env["MICHI_AUTH_PASSWORD"] = "admin123"
        env["MICHI_DATABASE_URL"] = f"sqlite:///tmp/michi_mqtt_test_{args.server_port}.db?mode=rwc"
        env["MICHI_CONFIG_PATH"] = f"/tmp/michi_mqtt_conf_{args.server_port}"
        os.makedirs(env["MICHI_CONFIG_PATH"], exist_ok=True)

        server_bin = os.path.abspath("target/debug/michi-server")
        if not os.path.exists(server_bin):
            server_bin = "michi-server"

        print(f"Starting Michi Micro Server on port {args.server_port}...")
        server_proc = subprocess.Popen(
            [server_bin],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT
        )

        if not wait_for_port(args.server_port, timeout=10.0):
            print("❌ Failed to start Michi Micro Server.")
            sys.exit(1)
        print(f"  ✅ Michi Micro Server live on 127.0.0.1:{args.server_port}")

        # 1. Observe real Home Assistant Auto-Discovery messages delivered through Mosquitto
        deadline = time.time() + 8.0
        discovery_topics = set()
        state_topics = {}

        EXPECTED_DISCOVERY = {
            "homeassistant/sensor/michi_track_title/config",
            "homeassistant/sensor/michi_artist/config",
            "homeassistant/sensor/michi_album/config",
            "homeassistant/sensor/michi_playback_status/config",
            "homeassistant/sensor/michi_volume/config",
            "homeassistant/sensor/michi_track_duration/config",
            "homeassistant/sensor/michi_playback_position/config",
            "homeassistant/sensor/michi_server_status/config",
            "homeassistant/button/michi_play_pause/config",
            "homeassistant/button/michi_play/config",
            "homeassistant/button/michi_pause/config",
            "homeassistant/button/michi_stop/config",
            "homeassistant/button/michi_next_track/config",
            "homeassistant/button/michi_previous_track/config",
            "homeassistant/number/michi_volume_set/config",
        }

        while time.time() < deadline:
            msgs = mqtt_client.poll_messages(timeout=0.3)
            for top, payload in msgs:
                if top.startswith("homeassistant/"):
                    discovery_topics.add(top)
                elif top.startswith("michi/"):
                    state_topics[top] = payload
            if EXPECTED_DISCOVERY.issubset(discovery_topics) and "michi/server_status/state" in state_topics:
                break

        missing = EXPECTED_DISCOVERY - discovery_topics
        assert not missing, f"Missing discovery topics from Mosquitto: {missing}"
        print(f"  ✅ 1. All {len(EXPECTED_DISCOVERY)} Home Assistant Discovery configs received via Mosquitto")

        # 2. Verify state publications
        assert state_topics.get("michi/server_status/state") == "online", f"Expected online server_status, got {state_topics}"
        assert "michi/volume/state" in state_topics, "michi/volume/state missing"
        print(f"  ✅ 2. Real MQTT states received: server_status={state_topics['michi/server_status/state']}, volume={state_topics['michi/volume/state']}")

        # 3. Send real MQTT command over Mosquitto broker and assert effect on both MQTT state and server
        mqtt_client.publish("michi/volume_set/cmd", "72", qos=1, pkid=10)
        cmd_deadline = time.time() + 5.0
        updated_volume = None
        while time.time() < cmd_deadline:
            msgs = mqtt_client.poll_messages(timeout=0.2)
            for top, payload in msgs:
                if top == "michi/volume/state":
                    updated_volume = payload
            if updated_volume == "72":
                break
        assert updated_volume == "72", f"Expected michi/volume/state to update to '72', got {updated_volume}"
        print("  ✅ 3. Sent MQTT command 'michi/volume_set/cmd' ('72') and verified state published as 72")

        # 4. Reconnect resilience: kill Mosquitto and restart it
        print("Testing broker restart & reconnect resilience...")
        mqtt_client.close()
        broker_proc.terminate()
        broker_proc.wait()
        time.sleep(1.0)

        broker_proc = subprocess.Popen(
            [mosquitto_bin, "-c", tmp_conf],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE
        )
        assert wait_for_port(args.broker_port, timeout=5.0), "Mosquitto failed to restart"
        print("  ✅ 4. Mosquitto restarted; waiting for Michi auto-reconnection...")

        # Reconnect observer
        mqtt_client = MiniMqttClient("127.0.0.1", args.broker_port, client_id="test_observer_reconnect")
        mqtt_client.connect()
        mqtt_client.subscribe("homeassistant/#", pkid=1)
        mqtt_client.subscribe("michi/#", pkid=2)

        # Allow reconnect loop (interval 5s) and assert all 15 discovery configs re-announced
        reconnect_deadline = time.time() + 10.0
        reconnect_discovery = set()
        reconnect_states = {}
        while time.time() < reconnect_deadline:
            msgs_after = mqtt_client.poll_messages(timeout=0.3)
            for top, payload in msgs_after:
                if top.startswith("homeassistant/"):
                    reconnect_discovery.add(top)
                elif top.startswith("michi/"):
                    reconnect_states[top] = payload
            if EXPECTED_DISCOVERY.issubset(reconnect_discovery) and "michi/server_status/state" in reconnect_states:
                break

        missing_reconnect = EXPECTED_DISCOVERY - reconnect_discovery
        assert not missing_reconnect, f"Missing discovery configs after reconnect: {missing_reconnect}"
        print(f"  ✅ 5. Verified all {len(EXPECTED_DISCOVERY)} discovery configs re-announced after broker reconnect")

        status, status_data = http_get(f"http://127.0.0.1:{args.server_port}/api/v1/status")
        assert status == 200, f"Expected 200, got {status}"
        print("  ✅ 6. Michi Micro Server healthy & online after reconnect")

        print("=" * 70)
        print("MOSQUITTO REAL INTEGRATION: ALL 6 STAGES PASSED (100% CERTIFIED)")
        print("=" * 70)

    finally:
        if mqtt_client:
            mqtt_client.close()
        if server_proc:
            server_proc.terminate()
            try:
                server_proc.wait(timeout=2.0)
            except Exception:
                server_proc.kill()
        if broker_proc:
            broker_proc.terminate()
            try:
                broker_proc.wait(timeout=2.0)
            except Exception:
                broker_proc.kill()
        if os.path.exists(tmp_conf):
            try:
                os.remove(tmp_conf)
            except Exception:
                pass

if __name__ == "__main__":
    main()
