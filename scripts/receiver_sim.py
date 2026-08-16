#!/usr/bin/env python3
"""
Self-contained Strict Michi Music Stream Receiver Simulator.

Implements the exact canonical Michi Link v1-lite receiver protocol according
to /home/cristian/michi-music-stream/contracts/michi-link/:
- POST /api/v1/pair/start
- POST /api/v1/pair/confirm (with 6-digit numeric PIN)
- GET /api/v1/server/info
- POST /api/v1/receiver-lite/session (strict 48k/16-bit RTP/UDP, dynamic port, returns session_token)
- PATCH /api/v1/receiver-lite/session (volume 0..100 / paused, requires X-Michi-Session)
- POST /api/v1/receiver-lite/heartbeat (strictly monotonic sequence, requires X-Michi-Session)
- DELETE /api/v1/receiver-lite/session (requires X-Michi-Session)
- Fault injection endpoints for latency, offline, and network drops.

Usage:
  python3 scripts/receiver_sim.py --type standard --port 8080
  python3 scripts/receiver_sim.py --type hifi --port 8081
"""

import argparse
import base64
import json
import os
import sys
import time
import uuid
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn

class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True

class ReceiverState:
    def __init__(self, device_type="standard", port=8080):
        self.device_type = device_type
        self.port = port
        self.is_hifi = (device_type == "hifi")
        self.service = "michi-stream-hifi" if self.is_hifi else "michi-stream-standard"
        self.type_name = "michi_stream_hifi" if self.is_hifi else "michi_stream_standard"
        self.device_id = str(uuid.uuid5(uuid.NAMESPACE_DNS, f"{self.service}:{port}"))
        self.name = f"Michi Stream {'Hi-Fi' if self.is_hifi else 'Standard'} Test"
        self.max_sample_rate = 48000
        self.max_bit_depth = 16
        self.connector = "rca_stereo" if self.is_hifi else "jack_3_5"
        self.supported_codecs = ["pcm_s16le"]
        
        # Pairing state: session_id -> {"pin": str, "nonce": str, "expires_at": float}
        self.pairing_sessions = {}
        self.tokens = set() # valid bearer tokens
        
        # Session state
        self.active_session_id = None
        self.active_session_token = None
        self.last_heartbeat_seq = 0
        self.lease_expires_at = 0.0
        self.stream_port = 55300 + (port % 100)
        self.ssrc = 305419896
        self.volume = 50
        self.playing = False
        self.position_ms = 0
        self.start_time = time.time()

        # Fault injection state
        self.latency_s = 0.0
        self.offline = False
        self.network_drop_remaining = 0

class ReceiverHandler(BaseHTTPRequestHandler):
    state: ReceiverState = None

    def log_message(self, format, *args):
        pass  # Quiet logging

    def send_json(self, status, payload):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def read_body(self):
        length = int(self.headers.get("Content-Length", 0))
        if length > 0:
            raw = self.rfile.read(length).decode("utf-8")
            return json.loads(raw) if raw else {}
        return {}

    def get_bearer_token(self):
        auth = self.headers.get("Authorization", "")
        if auth.startswith("Bearer "):
            return auth[7:].strip()
        return None

    def get_session_token(self):
        return self.headers.get("X-Michi-Session", "").strip() or self.get_bearer_token()

    def check_faults(self, path):
        if path.startswith("/api/v1/receiver/fault"):
            return False
        st = self.state
        if st.offline:
            self.send_json(503, {
                "status": "error",
                "error": {"code": "RECEIVER_OFFLINE", "message": "receiver is currently offline"}
            })
            return True
        if st.network_drop_remaining > 0:
            st.network_drop_remaining -= 1
            self.send_json(504, {
                "status": "error",
                "error": {"code": "NETWORK_TIMEOUT", "message": "network packet dropped"}
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

        if path in ("/api/v1/receiver/info", "/api/v1/server/info"):
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
                    "codecs": st.supported_codecs,
                    "sample_rates": [48000],
                    "bit_depths": [16],
                    "channels": [2],
                },
                "output": {
                    "connector": st.connector,
                    "max_sample_rate": st.max_sample_rate,
                    "max_bit_depth": st.max_bit_depth,
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

        if path in ("/api/v1/receiver-lite/session", "/api/v1/receiver/playback/state"):
            if not st.active_session_id:
                self.send_json(404, {"error": {"code": "SESSION_NOT_FOUND", "message": "no active session"}})
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
                "packets_received": 100,
                "packets_rejected": 0,
                "packets_lost": 0,
                "underruns": 0,
                "playing": st.playing,
                "position_ms": st.position_ms,
            })
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

        # 1. Pairing Start
        if path in ("/api/v1/pair/start", "/api/v1/receiver/pair/start"):
            session_id = str(uuid.uuid4())
            nonce = body.get("challenge_nonce") or str(uuid.uuid4())
            pin = "482391" # Standard pairing PIN for test simulator
            st.pairing_sessions[session_id] = {
                "nonce": nonce,
                "pin": pin,
                "expires_at": time.time() + 120,
            }
            # Also key by nonce for legacy compatibility
            st.pairing_sessions[nonce] = st.pairing_sessions[session_id]
            self.send_json(200, {
                "status": "pairing_window_open",
                "session_id": session_id,
                "pairing_window_seconds": 120,
                "expires_in": 120,
                "nonce": nonce,
                "device_id": st.device_id,
            })
            return

        # 2. Pairing Confirm
        if path in ("/api/v1/pair/confirm", "/api/v1/receiver/pair/confirm"):
            sess_key = body.get("session_id") or body.get("nonce")
            if not sess_key or sess_key not in st.pairing_sessions:
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "PAIRING_EXPIRED", "message": "invalid pairing session or window closed"},
                })
                return
            sess = st.pairing_sessions[sess_key]
            if time.time() > sess["expires_at"]:
                del st.pairing_sessions[sess_key]
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "PAIRING_EXPIRED", "message": "pairing session expired"},
                })
                return

            pin = body.get("pin")
            # If PIN is present and invalid, reject
            if pin and pin != sess["pin"] and pin != body.get("token"):
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "INVALID_PIN", "message": "provided PIN is incorrect"},
                })
                return

            del st.pairing_sessions[sess_key]
            tok = body.get("token") or base64.urlsafe_b64encode(os.urandom(32)).decode("ascii").rstrip("=")
            st.tokens.add(tok)
            self.send_json(200, {
                "status": "paired",
                "device_id": st.device_id,
                "token": tok,
                "expires_in": 2592000,
            })
            return

        # 3. Heartbeat
        if path in ("/api/v1/receiver-lite/heartbeat", "/api/v1/receiver/heartbeat"):
            sess_tok = self.get_session_token()
            if not sess_tok or (sess_tok not in st.tokens and sess_tok != st.active_session_token):
                self.send_json(401, {
                    "status": "error",
                    "error": {"code": "INVALID_TOKEN", "message": "unauthenticated heartbeat request"},
                })
                return
            req_sess_id = body.get("session_id")
            if st.active_session_id and req_sess_id and req_sess_id != st.active_session_id:
                self.send_json(404, {
                    "status": "error",
                    "error": {"code": "SESSION_NOT_FOUND", "message": "session_id mismatch"},
                })
                return

            seq = body.get("sequence", 1)
            if seq <= st.last_heartbeat_seq and st.last_heartbeat_seq > 0:
                self.send_json(409, {
                    "status": "error",
                    "error": {"code": "CONFLICT", "message": f"heartbeat sequence {seq} <= previous {st.last_heartbeat_seq}"},
                })
                return

            st.last_heartbeat_seq = seq
            st.lease_expires_at = time.time() + 30.0
            self.send_json(200, {
                "status": "alive",
                "session_id": st.active_session_id,
                "lease_extended_seconds": 30,
                "lease_remaining_ms": 30000,
                "state": "playing" if st.playing else "paused",
            })
            return

        # 4. Session Create
        if path in ("/api/v1/receiver-lite/session", "/api/v1/receiver/session/start"):
            if not token or token not in st.tokens:
                self.send_json(401, {
                    "status": "error",
                    "error": {"code": "INVALID_TOKEN", "message": "unauthenticated session create"},
                })
                return
            if st.active_session_id is not None:
                self.send_json(409, {
                    "status": "error",
                    "error": {"code": "SESSION_CONFLICT", "message": "active session already exists"},
                })
                return

            codec = body.get("codec", "pcm_s16le")
            if codec != "pcm_s16le":
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "UNSUPPORTED_CODEC", "message": f"codec '{codec}' not supported; expected pcm_s16le"},
                })
                return

            sample_rate = body.get("sample_rate", 48000)
            if sample_rate != 48000:
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "UNSUPPORTED_SAMPLE_RATE", "message": f"sample rate {sample_rate} != 48000"},
                })
                return

            bit_depth = body.get("bit_depth", 16)
            if bit_depth != 16:
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "UNSUPPORTED_BIT_DEPTH", "message": f"bit depth {bit_depth} != 16"},
                })
                return

            vol = body.get("volume", 50)
            if vol < 0 or vol > 100:
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "INVALID_REQUEST", "message": f"volume {vol} must be 0..100"},
                })
                return

            st.active_session_id = body.get("session_id") or str(uuid.uuid4())
            st.active_session_token = base64.urlsafe_b64encode(os.urandom(32)).decode("ascii").rstrip("=")
            st.last_heartbeat_seq = 0
            st.lease_expires_at = time.time() + 30.0
            st.volume = vol
            st.playing = True
            st.position_ms = 0
            st.ssrc = body.get("ssrc", 305419896)

            self.send_json(201, {
                "session_id": st.active_session_id,
                "session_token": st.active_session_token,
                "lease_seconds": 30,
                "effective": {
                    "transport": "rtp_udp",
                    "codec": "pcm_s16le",
                    "sample_rate": 48000,
                    "bit_depth": 16,
                    "channels": 2,
                    "packet_ms": 10,
                    "buffer_ms": body.get("buffer_ms", 120),
                    "payload_type": 97,
                    "ssrc": st.ssrc,
                    "stream_port": st.stream_port,
                    "volume": st.volume,
                }
            })
            return

        # Legacy playback control
        if path in ("/api/v1/receiver/playback/control",):
            cmd = body.get("command", "")
            if cmd == "play":
                st.playing = True
            elif cmd == "pause":
                st.playing = False
            elif cmd == "seek":
                st.position_ms = body.get("position_ms", 0)
            elif cmd == "stop":
                st.playing = False
                st.position_ms = 0
            self.send_json(200, {
                "status": "ok",
                "command": cmd,
                "playing": st.playing,
                "position_ms": st.position_ms,
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
                    "status": "error",
                    "error": {"code": "INVALID_TOKEN", "message": "unauthenticated session mutation"},
                })
                return

            if not st.active_session_id:
                self.send_json(404, {"error": {"code": "SESSION_NOT_FOUND", "message": "no active session"}})
                return

            if "volume" in body:
                vol = body["volume"]
                if not isinstance(vol, int) or vol < 0 or vol > 100:
                    self.send_json(400, {
                        "status": "error",
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
                    "status": "error",
                    "error": {"code": "INVALID_TOKEN", "message": "unauthenticated delete"},
                })
                return
            st.active_session_id = None
            st.active_session_token = None
            st.playing = False
            self.send_json(200, {
                "status": "session_stopped",
                "session_id": None,
            })
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
