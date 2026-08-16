#!/usr/bin/env python3
"""
Self-contained Michi Music Stream Receiver Simulator.

Provides standard and hifi receiver endpoints for both canonical v1-lite
and legacy /api/v1/receiver/* routes for integration testing.

Usage:
  python3 scripts/receiver_sim.py --type standard --port 8080
  python3 scripts/receiver_sim.py --type hifi --port 8081
"""

import argparse
import json
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
        self.max_sample_rate = 96000 if self.is_hifi else 48000
        self.max_bit_depth = 24 if self.is_hifi else 16
        self.connector = "rca_stereo" if self.is_hifi else "jack_3_5"
        self.supported_codecs = ["pcm_s24le", "pcm_s16le"] if self.is_hifi else ["pcm_s16le"]
        
        self.nonces = {} # nonce -> expires_at
        self.tokens = set()
        self.active_session_id = None
        self.volume = 50
        self.start_time = time.time()

class ReceiverHandler(BaseHTTPRequestHandler):
    state: ReceiverState = None

    def log_message(self, format, *args):
        pass  # Quiet logging for test execution

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

    def get_token(self):
        auth = self.headers.get("Authorization", "")
        if auth.startswith("Bearer "):
            return auth[7:].strip()
        return None

    def do_GET(self):
        st = self.state
        path = self.path.split("?")[0]

        if path in ("/api/v1/receiver/info", "/api/v1/server/info"):
            self.send_json(200, {
                "service": st.service,
                "name": st.name,
                "device_id": st.device_id,
                "id": st.device_id,
                "api_version": "v1-lite",
                "type": st.type_name,
                "roles": ["audio_receiver"],
                "supported_codecs": st.supported_codecs,
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
                },
            })
            return

        self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "not found"}})

    def do_POST(self):
        st = self.state
        path = self.path.split("?")[0]
        body = self.read_body()
        token = self.get_token()

        if path in ("/api/v1/receiver/pair/start", "/api/v1/pair/start"):
            nonce = str(uuid.uuid4())
            st.nonces[nonce] = time.time() + 120
            self.send_json(200, {
                "status": "pairing_window_open",
                "pairing_window_seconds": 120,
                "nonce": nonce,
                "device_id": st.device_id,
            })
            return

        if path in ("/api/v1/receiver/pair/confirm", "/api/v1/pair/confirm"):
            nonce = body.get("nonce")
            if not nonce or nonce not in st.nonces or time.time() > st.nonces[nonce]:
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "pairing_failed", "message": "invalid nonce or window closed"},
                })
                return
            del st.nonces[nonce]
            tok = body.get("token", str(uuid.uuid4()))
            st.tokens.add(tok)
            self.send_json(200, {
                "status": "paired",
                "device_id": st.device_id,
                "token": tok,
            })
            return

        if path in ("/api/v1/receiver/heartbeat", "/api/v1/receiver-lite/heartbeat"):
            if not token or token not in st.tokens:
                self.send_json(401, {
                    "status": "error",
                    "error": {"code": "invalid_token", "message": "unauthenticated"},
                })
                return
            self.send_json(200, {
                "status": "alive",
                "session_id": st.active_session_id,
                "uptime_seconds": int(time.time() - st.start_time),
            })
            return

        if path in ("/api/v1/receiver/session/start", "/api/v1/receiver-lite/session"):
            if not token or token not in st.tokens:
                self.send_json(401, {
                    "status": "error",
                    "error": {"code": "invalid_token", "message": "unauthenticated"},
                })
                return
            if st.active_session_id is not None:
                self.send_json(409, {
                    "status": "error",
                    "error": {"code": "session_conflict", "message": "active session already exists"},
                })
                return
            codec = body.get("codec", "")
            if codec not in st.supported_codecs:
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "unsupported_codec", "message": f"codec '{codec}' not supported"},
                })
                return
            sample_rate = body.get("sample_rate", 0)
            if sample_rate > st.max_sample_rate:
                self.send_json(400, {
                    "status": "error",
                    "error": {"code": "sample_rate_exceeds_max", "message": f"sample rate {sample_rate} exceeds max {st.max_sample_rate}"},
                })
                return
            st.active_session_id = body.get("session_id", str(uuid.uuid4()))
            st.volume = min(100, body.get("volume", 70))
            self.send_json(200, {
                "status": "session_started",
                "session_id": st.active_session_id,
                "device_id": st.device_id,
                "stream_port": body.get("stream_port", 55300),
                "buffer_ms": body.get("buffer_ms", 250),
            })
            return

        if path in ("/api/v1/receiver/session/stop",):
            st.active_session_id = None
            self.send_json(200, {
                "status": "session_stopped",
                "session_id": None,
            })
            return

        if path in ("/api/v1/receiver/volume",):
            vol = body.get("volume", 50)
            st.volume = min(100, max(0, vol))
            self.send_json(200, {
                "status": "volume_updated",
                "volume": st.volume,
            })
            return

        self.send_json(404, {"error": {"code": "NOT_FOUND", "message": "not found"}})

def run_server(device_type, port, host="0.0.0.0"):
    state = ReceiverState(device_type=device_type, port=port)
    handler = type("ConfiguredReceiverHandler", (ReceiverHandler,), {"state": state})
    server = ThreadedHTTPServer((host, port), handler)
    print(f"Michi Receiver Simulator ({device_type}) running on http://{host}:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

def main():
    parser = argparse.ArgumentParser(description="Michi Receiver Simulator")
    parser.add_argument("--type", choices=["standard", "hifi"], default="standard")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--host", default="0.0.0.0")
    args = parser.parse_args()
    run_server(args.type, args.port, args.host)

if __name__ == "__main__":
    main()
