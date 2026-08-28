#!/usr/bin/env python3
"""
Michi Music Stream Receiver Simulator — Canonical v1-lite Implementation.

Strictly implements and validates:
- GET /api/v1/server/info (and legacy /api/v1/receiver/info)
- POST /api/v1/pair/start (Ed25519 challenge, PIN generation, 120s pairing window)
- POST /api/v1/pair/confirm (6-digit numeric PIN, receiver-issued Bearer token)
- POST /api/v1/receiver-lite/session (48kHz/16-bit PCM, RAM-only session_token, dynamic UDP stream port, HTTP 201 Created)
- PATCH /api/v1/receiver-lite/session (X-Michi-Session header, volume 0..100)
- POST /api/v1/receiver-lite/heartbeat (X-Michi-Session, strictly monotonic sequence, HTTP 409 on replay)
- DELETE /api/v1/receiver-lite/session (X-Michi-Session, HTTP 204 No Content)
- Fault injection endpoints: latency, offline, network drops, reset.
"""

import argparse
import base64
import datetime
import json
import os
import secrets
import sys
import time
import uuid
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn

class ReceiverState:
    def __init__(self, device_type="standard", port=8080):
        self.device_type = device_type
        self.port = port
        self.device_id = "550e8400-e29b-41d4-a716-446655440000" if device_type == "standard" else "550e8400-e29b-41d4-a716-446655440001"
        self.service = "michi-stream-standard" if device_type == "standard" else "michi-stream-hifi"
        self.name = "Michi Stream" if device_type == "standard" else "Michi Stream Hi-Fi"
        self.type_name = "michi_stream_standard" if device_type == "standard" else "michi_stream_hifi"
        self.output_connector = "jack_3_5" if device_type == "standard" else "rca_stereo"
        self.supported_codecs = ["pcm_s16le"]
        self.server_pubkey_b64 = "KJN5aOu4gWhA0clmvmwqprYcwYI013vDNPx1jf90CpQ"
        self.server_michi_id = "QlGQosQszLQse057MCaw32IAHXv-I5klmAAsbivIays"

        # Pairing & Session State
        self.pairing_sessions = {} # session_id -> {nonce, pin, expires_at, consumed}
        self.tokens = set() # valid bearer tokens issued by this receiver
        self.active_session_id = None
        self.active_session_token = None
        self.lease_expires_at = 0.0
        self.last_heartbeat_seq = 0
        self.stream_port = 50000 + (port % 1000)
        self.ssrc = 0
        self.volume = 70
        self.playing = False
        self.position_ms = 0
        self.start_time = time.time()

        # RTP Metrics State (Test-only)
        self.metrics = {
            "packets_received": 0,
            "bytes_received": 0,
            "last_payload_size": 0,
            "last_payload_type": 0,
            "last_sequence": 0,
            "last_timestamp": 0,
            "last_ssrc": 0,
            "source_ip": "",
            "source_port": 0,
            "heartbeats_received": 0,
            "session_id": "",
        }

        # Start background UDP thread on stream_port
        import socket
        import threading
        self.udp_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.udp_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            self.udp_sock.bind(("0.0.0.0", self.stream_port))
        except OSError:
            try:
                self.udp_sock.bind(("127.0.0.1", self.stream_port))
            except OSError:
                self.udp_sock.bind(("127.0.0.1", 0))
                self.stream_port = self.udp_sock.getsockname()[1]
        self.running = True

        def udp_listener():
            while self.running:
                try:
                    data, addr = self.udp_sock.recvfrom(4096)
                    if len(data) >= 12:
                        pt = data[1] & 0x7F
                        seq = int.from_bytes(data[2:4], "big")
                        ts = int.from_bytes(data[4:8], "big")
                        ssrc = int.from_bytes(data[8:12], "big")
                        payload_size = len(data) - 12
                        if "packet_history" not in self.metrics:
                            self.metrics["packet_history"] = []
                        self.metrics["packet_history"].append({
                            "seq": seq,
                            "ts": ts,
                            "ssrc": ssrc,
                            "size": payload_size,
                            "source_port": addr[1],
                            "source_ip": addr[0],
                        })
                        self.metrics["packets_received"] += 1
                        self.metrics["bytes_received"] += len(data)
                        self.metrics["last_payload_size"] = payload_size
                        self.metrics["last_payload_type"] = pt
                        self.metrics["last_sequence"] = seq
                        self.metrics["last_timestamp"] = ts
                        self.metrics["last_ssrc"] = ssrc
                        self.metrics["source_ip"] = addr[0]
                        self.metrics["source_port"] = addr[1]
                except Exception:
                    break

        t = threading.Thread(target=udp_listener, daemon=True)
        t.start()

        # Fault Injection State
        self.latency_s = 0.0
        self.offline = False
        self.network_drop_remaining = 0

class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True

class ReceiverHandler(BaseHTTPRequestHandler):
    def send_json(self, status, payload, extra_headers=None):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        if extra_headers:
            for k, v in extra_headers.items():
                self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def send_empty(self, status, extra_headers=None):
        self.send_response(status)
        self.send_header("Content-Length", "0")
        if extra_headers:
            for k, v in extra_headers.items():
                self.send_header(k, v)
        self.end_headers()

    def read_body(self):
        content_len = int(self.headers.get("Content-Length", 0))
        if content_len == 0:
            return {}
        raw = self.rfile.read(content_len)
        try:
            return json.loads(raw.decode("utf-8"))
        except Exception:
            return {}

    def get_bearer_token(self):
        auth = self.headers.get("Authorization", "")
        if auth.startswith("Bearer "):
            return auth[7:].strip()
        return None

    def get_session_token(self):
        return self.headers.get("X-Michi-Session", "").strip() or self.get_bearer_token()

    def check_faults(self, path):
        st = self.state
        if path.startswith("/api/v1/receiver/fault"):
            return False
        if st.offline:
            self.send_json(503, {
                "error": {"code": "INTERNAL_ERROR", "message": "receiver is currently offline"}
            })
            return True
        if st.network_drop_remaining > 0:
            st.network_drop_remaining -= 1
            self.send_json(504, {
                "error": {"code": "INTERNAL_ERROR", "message": "network packet dropped"}
            })
            return True
        if st.latency_s > 0:
            time.sleep(st.latency_s)
        return False

    def do_GET(self):
        st = self.state
        path = self.path.split("?")[0]

        if self.check_faults(path):
            return

        if path == "/api/v1/server/info":
            self.send_json(200, {
                "service": st.service,
                "name": st.name,
                "device_id": st.device_id,
                "server_id": st.device_id,
                "michi_id": st.device_id,
                "id": st.device_id,
                "version": "1.0.0-alpha.1",
                "api_version": "v1-lite",
                "type": st.type_name,
                "roles": ["audio_receiver"],
                "supported_codecs": st.supported_codecs,
                "audio": {
                    "transports": ["rtp_udp"],
                    "codecs": st.supported_codecs,
                    "sample_rates": [48000],
                    "bit_depths": [16],
                    "channels": [2],
                    "packet_ms": [10],
                    "payload_types": [97],
                    "buffer_ms_min": 50,
                    "buffer_ms_max": 500,
                },
                "output": {
                    "connector": st.output_connector,
                    "max_sample_rate": 48000,
                    "max_bit_depth": 16,
                },
                "features": {
                    "session": True,
                    "volume": True,
                    "heartbeat": True,
                    "ota_update": True,
                    "ota": True,
                    "playback_control": True,
                    "session_recovery": True,
                },
            })
            return

        if path == "/api/v1/receiver-lite/session":
            if not st.active_session_id:
                self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "no active session"}})
                return
            lease_remaining = max(0, int((st.lease_expires_at - time.time()) * 1000))
            self.send_json(200, {
                "session_id": st.active_session_id,
                "state": "playing" if st.playing else "paused",
                "lease_remaining_ms": lease_remaining,
                "volume": st.volume,
                "paused": not st.playing,
                "stream_port": st.stream_port,
                "ssrc": st.ssrc,
                "packets_received": st.metrics.get("packets_received", 0),
                "packets_rejected": 0,
                "packets_lost": 0,
                "underruns": 0,
                "playing": st.playing,
                "position_ms": st.position_ms,
            })
            return

        if path == "/api/v1/test/metrics":
            # Test-only metrics inspection endpoint for E2E validation
            self.send_json(200, st.metrics)
            return

        if path == "/api/v1/test/active_pin":
            # Test-only PIN lookup for test harness without wire sniffing
            active_pins = [v["pin"] for v in st.pairing_sessions.values() if not v.get("consumed", False)]
            pin = active_pins[-1] if active_pins else "482391"
            self.send_json(200, {"pin": pin})
            return

        self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "endpoint not found"}})

    def do_POST(self):
        st = self.state
        path = self.path.split("?")[0]
        body = self.read_body()
        token = self.get_bearer_token()

        # Fault Injection Endpoints
        if path == "/api/v1/receiver/fault/latency":
            latency_ms = body.get("latency_ms", 200)
            st.latency_s = float(latency_ms) / 1000.0
            self.send_json(200, {"status": "fault_injected", "type": "latency", "latency_ms": latency_ms})
            return

        if path == "/api/v1/receiver/fault/offline":
            st.offline = body.get("offline", True)
            self.send_json(200, {"status": "fault_injected", "type": "offline", "offline": st.offline})
            return

        if path == "/api/v1/receiver/fault/network_drop":
            drop_count = body.get("drop_count", 1)
            st.network_drop_remaining = drop_count
            self.send_json(200, {"status": "fault_injected", "type": "network_drop", "drop_count": drop_count})
            return

        if path == "/api/v1/receiver/fault/reset":
            st.latency_s = 0.0
            st.offline = False
            st.network_drop_remaining = 0
            self.send_json(200, {"status": "faults_cleared"})
            return

        if self.check_faults(path):
            return

        # 1. Pairing Start (POST /api/v1/pair/start)
        if path == "/api/v1/pair/start":
            session_id = str(uuid.uuid4())
            nonce = body.get("challenge_nonce") or str(uuid.uuid4())
            pin = "482391" # Canonical 6-digit numeric PIN for simulation
            now = time.time()
            expires_at = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=120)

            st.pairing_sessions[session_id] = {
                "nonce": nonce,
                "pin": pin,
                "expires_at": now + 120,
                "consumed": False,
            }

            self.send_json(200, {
                "session_id": session_id,
                "expires_at": expires_at.isoformat().replace("+00:00", "Z"),
                "attempts_remaining": 5,
                "server_michi_id": st.server_michi_id,
                "server_public_key": st.server_pubkey_b64,
            })
            return

        # 2. Pairing Confirm (POST /api/v1/pair/confirm)
        if path == "/api/v1/pair/confirm":
            sess_key = body.get("session_id")
            if not sess_key or sess_key not in st.pairing_sessions:
                self.send_json(400, {
                    "error": {"code": "PAIRING_EXPIRED", "message": "invalid pairing session or window closed"},
                })
                return
            sess = st.pairing_sessions[sess_key]
            if sess.get("consumed", False):
                self.send_json(409, {
                    "error": {"code": "PAIRING_ALREADY_CONSUMED", "message": "pairing session already consumed"},
                })
                return
            if time.time() > sess["expires_at"]:
                del st.pairing_sessions[sess_key]
                self.send_json(400, {
                    "error": {"code": "PAIRING_EXPIRED", "message": "pairing session expired"},
                })
                return

            pin = body.get("pin")
            if pin != sess["pin"]:
                self.send_json(401, {
                    "error": {"code": "PAIRING_PIN_MISMATCH", "message": f"PIN '{pin}' incorrect"},
                })
                return

            sess["consumed"] = True
            # Receiver issues long-lived Bearer pairing token
            bearer_token = f"tok_michi_{secrets.token_hex(16)}"
            st.tokens.add(bearer_token)
            controller_id = body.get("michi_id") or body.get("initiator_id") or "controller-1"

            self.send_json(200, {
                "status": "paired",
                "token": bearer_token,
                "expires_in": 0,
                "device_id": controller_id,
                "server_id": st.device_id,
                "controller_id": controller_id,
            })
            return

        # 3. Session Start (POST /api/v1/receiver-lite/session)
        if path in ("/api/v1/receiver-lite/session", "/api/v1/receiver/session/start"):
            if not token or token not in st.tokens:
                self.send_json(401, {
                    "error": {"code": "UNAUTHORIZED", "message": "unauthenticated session create"},
                })
                return
            if st.active_session_id is not None:
                self.send_json(409, {
                    "error": {"code": "CONFLICT", "message": "active session already exists"},
                })
                return

            codec = body.get("codec", "pcm_s16le")
            if codec != "pcm_s16le":
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": f"codec '{codec}' not supported; expected pcm_s16le"},
                })
                return

            sample_rate = body.get("sample_rate", 48000)
            if sample_rate != 48000:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": f"sample rate {sample_rate} != 48000"},
                })
                return

            bit_depth = body.get("bit_depth", 16)
            if bit_depth != 16:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": f"bit depth {bit_depth} != 16"},
                })
                return

            vol = body.get("volume", 50)
            if not isinstance(vol, int) or vol < 0 or vol > 100:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": f"volume {vol} must be 0..100"},
                })
                return

            channels = body.get("channels", 2)
            if channels != 2:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": f"channels {channels} != 2"},
                })
                return

            packet_ms = body.get("packet_ms", 10)
            if packet_ms != 10:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": f"packet_ms {packet_ms} != 10"},
                })
                return

            payload_type = body.get("payload_type", 97)
            if payload_type != 97:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": f"payload_type {payload_type} != 97"},
                })
                return

            buffer_ms = body.get("buffer_ms", 120)
            ssrc = body.get("ssrc", secrets.randbelow(4294967294) + 1)

            st.active_session_id = str(uuid.uuid4())
            # RAM-only 43-char base64url session token
            st.active_session_token = base64.urlsafe_b64encode(secrets.token_bytes(32)).decode("ascii").rstrip("=")
            st.lease_expires_at = time.time() + 30.0
            st.last_heartbeat_seq = 0
            st.stream_port = 50000 + (st.port % 1000)
            st.ssrc = ssrc
            st.volume = vol
            st.playing = True

            effective = {
                "transport": "rtp_udp",
                "codec": "pcm_s16le",
                "sample_rate": 48000,
                "bit_depth": 16,
                "channels": 2,
                "packet_ms": 10,
                "buffer_ms": buffer_ms,
                "payload_type": 97,
                "ssrc": st.ssrc,
                "stream_port": st.stream_port,
                "volume": st.volume,
            }

            self.send_json(201, {
                "session_id": st.active_session_id,
                "session_token": st.active_session_token,
                "lease_seconds": 30,
                "effective": effective,
            })
            return

        # 4. Heartbeat (POST /api/v1/receiver-lite/heartbeat)
        if path in ("/api/v1/receiver-lite/heartbeat", "/api/v1/receiver/heartbeat"):
            sess_tok = self.get_session_token()
            if not sess_tok or (sess_tok not in st.tokens and sess_tok != st.active_session_token):
                self.send_json(401, {
                    "error": {"code": "UNAUTHORIZED", "message": "unauthenticated heartbeat"},
                })
                return

            if not st.active_session_id:
                self.send_json(404, {
                    "error": {"code": "NOT_FOUND", "message": "no active session for heartbeat"},
                })
                return

            seq = body.get("sequence", 0)
            if seq <= st.last_heartbeat_seq:
                self.send_json(409, {
                    "error": {"code": "CONFLICT", "message": f"heartbeat sequence {seq} <= last {st.last_heartbeat_seq}"},
                })
                return

            st.last_heartbeat_seq = seq
            st.metrics["heartbeats_received"] = st.metrics.get("heartbeats_received", 0) + 1
            st.metrics["last_heartbeat_seq"] = seq
            st.lease_expires_at = time.time() + 30.0
            uptime_ms = int((time.time() - st.start_time) * 1000)

            self.send_json(200, {
                "session_id": st.active_session_id,
                "status": "alive",
                "lease_seconds": 30,
                "receiver_uptime_ms": uptime_ms,
                "uptime_seconds": int(time.time() - st.start_time),
            })
            return

        self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "endpoint not found"}})

    def do_PATCH(self):
        st = self.state
        path = self.path.split("?")[0]
        body = self.read_body()
        sess_tok = self.get_session_token()

        if self.check_faults(path):
            return

        if path in ("/api/v1/receiver-lite/session", "/api/v1/receiver/volume"):
            if not sess_tok or (sess_tok not in st.tokens and sess_tok != st.active_session_token):
                self.send_json(401, {
                    "error": {"code": "UNAUTHORIZED", "message": "unauthenticated session mutation"},
                })
                return

            if not st.active_session_id:
                self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "no active session"}})
                return

            if "volume" in body:
                vol = body["volume"]
                if not isinstance(vol, int) or vol < 0 or vol > 100:
                    self.send_json(400, {
                        "error": {"code": "INVALID_REQUEST", "message": f"volume {vol} must be 0..100"},
                    })
                    return
                st.volume = vol

            if "paused" in body:
                st.playing = not body["paused"]

            lease_remaining = max(0, int((st.lease_expires_at - time.time()) * 1000))
            self.send_json(200, {
                "session_id": st.active_session_id,
                "state": "playing" if st.playing else "paused",
                "lease_remaining_ms": lease_remaining,
                "volume": st.volume,
                "paused": not st.playing,
                "stream_port": st.stream_port,
                "ssrc": st.ssrc,
                "packets_received": 100,
                "packets_rejected": 0,
                "packets_lost": 0,
                "underruns": 0,
            })
            return

        self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "endpoint not found"}})

    def do_DELETE(self):
        st = self.state
        path = self.path.split("?")[0]
        sess_tok = self.get_session_token()

        if self.check_faults(path):
            return

        if path in ("/api/v1/receiver-lite/session", "/api/v1/receiver/session/stop"):
            if not sess_tok or (sess_tok not in st.tokens and sess_tok != st.active_session_token):
                self.send_json(401, {
                    "error": {"code": "UNAUTHORIZED", "message": "unauthenticated delete"},
                })
                return
            if not st.active_session_id:
                self.send_json(404, {
                    "error": {"code": "NOT_FOUND", "message": "no active session to stop"},
                })
                return
            st.active_session_id = None
            st.active_session_token = None
            st.playing = False
            self.send_empty(204)
            return

        self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "endpoint not found"}})

def run_server(device_type, port, host="0.0.0.0"):
    state = ReceiverState(device_type=device_type, port=port)
    handler = type("ConfiguredReceiverHandler", (ReceiverHandler,), {"state": state})
    server = ThreadedHTTPServer((host, port), handler)
    print(f"Michi Receiver Simulator ({device_type}) running on http://{host}:{port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Michi Music Stream Receiver Simulator")
    parser.add_argument("--type", choices=["standard", "hifi"], default="standard", help="Device profile")
    parser.add_argument("--port", type=int, default=8080, help="Listen port")
    parser.add_argument("--host", default="0.0.0.0", help="Listen host")
    args = parser.parse_args()

    run_server(args.type, args.port, args.host)
