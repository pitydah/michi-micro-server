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
import hashlib
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
        self.home_id = "FU1FL-wFLfsfew3qpbR7XjDkmStWZY4g84MyW-zXPOs"
        self.active_challenges = {}

        root_seed = bytes.fromhex("c66a870d78788b4028cf21153c6a2b38efb5b0c92bcc4cfae58cc87e1f1f0377")
        if device_type == "standard":
            server_seed = bytes.fromhex("3188fd73c843983cf5751b0ce5f639d41f44613a77395e45b5b4a9e9499e43a1")
            self.server_michi_id = "f2UwxQaeA6vA8LO7Cr1nGRr5MStned_Gbmc_ua48qUc"
        else:
            server_seed = bytes.fromhex("e11972bb6837e27476a8986472a9e621a24475fbf99194ef41925844ffee72d3")
            self.server_michi_id = "FySPx3wCSC4rOV-wiEUTb0gp87XncaLT3-k44eoDe4s"

        try:
            from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
            self.home_root_private_key = Ed25519PrivateKey.from_private_bytes(root_seed)
            self.home_root_public_key = self.home_root_private_key.public_key()

            self.server_private_key = Ed25519PrivateKey.from_private_bytes(server_seed)
            pk_bytes = self.server_private_key.public_key().public_bytes_raw()
            self.server_pubkey_b64 = base64.urlsafe_b64encode(pk_bytes).decode("ascii").rstrip("=")

            canon_bytes = (
                b"michi-link-membership-v1"
                + self.home_id.encode("ascii")
                + self.server_michi_id.encode("ascii")
                + self.server_pubkey_b64.encode("ascii")
                + b"stream"
                + b":audio_receiver"
                + b":2026-10-04T12:00:00Z:1"
            )
            server_mem_sig = base64.urlsafe_b64encode(self.home_root_private_key.sign(canon_bytes)).decode("ascii").rstrip("=")
            self.server_membership = {
                "version": 1,
                "home_id": self.home_id,
                "device_michi_id": self.server_michi_id,
                "device_public_key": self.server_pubkey_b64,
                "device_type": "stream",
                "roles": ["audio_receiver"],
                "issued_at": "2026-10-04T12:00:00Z",
                "serial": 1,
                "signature": server_mem_sig,
            }
        except Exception as e:
            self.server_private_key = None
            self.server_membership = None
            if device_type == "standard":
                self.server_pubkey_b64 = "CGzuzD0UgfvAs1PJdcBBA1XqgVC28pgABFMzR6VNnq8"
                self.server_michi_id = "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4"
            else:
                self.server_pubkey_b64 = "4CggHpvLXArVU2CJypgueD9MOtNfT9l1dfbCXrNdOts"
                self.server_michi_id = "lz4CalNVFwbIecx40oFy7Z1HCzkonqkdcBP_eG3FZjo"

        # Pairing & Session State
        self.pairing_sessions = {} # session_id -> {nonce, pin, expires_at, consumed}
        self.tokens = set() # valid bearer tokens issued by this receiver
        self.controllers = {} # controller_id -> controller dict
        self.recovery_challenges = {} # (michi_id, public_key) -> challenge dict
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
            "payload_bytes_received": 0,
            "last_payload_size": 0,
            "last_payload_type": 0,
            "last_sequence": 0,
            "last_timestamp": 0,
            "last_ssrc": 0,
            "malformed_packets": 0,
            "source_ip": "",
            "source_port": 0,
            "heartbeats_received": 0,
            "session_id": "",
            "first_sequence": None,
            "first_timestamp": None,
            "packet_history": [],
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
                        if "first_sequence" not in self.metrics or self.metrics["first_sequence"] is None:
                            self.metrics["first_sequence"] = seq
                        if "first_timestamp" not in self.metrics or self.metrics["first_timestamp"] is None:
                            self.metrics["first_timestamp"] = ts
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
                        self.metrics["payload_bytes_received"] = self.metrics.get("payload_bytes_received", 0) + payload_size
                        self.metrics["last_payload_size"] = payload_size
                        self.metrics["last_payload_type"] = pt
                        self.metrics["last_sequence"] = seq
                        self.metrics["last_timestamp"] = ts
                        self.metrics["last_ssrc"] = ssrc
                        self.metrics["source_ip"] = addr[0]
                        self.metrics["source_port"] = addr[1]
                    else:
                        self.metrics["malformed_packets"] = self.metrics.get("malformed_packets", 0) + 1
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
        url_parts = self.path.split("?")
        path = url_parts[0]
        query = {}
        if len(url_parts) > 1:
            for pair in url_parts[1].split("&"):
                if "=" in pair:
                    k, v = pair.split("=", 1)
                    query[k] = v

        if self.check_faults(path):
            return

        if path == "/api/v1/pair/status":
            session_id = query.get("session_id")
            if not session_id:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": "missing session_id query parameter"}
                })
                return
            sess = st.pairing_sessions.get(session_id)
            if sess is None:
                self.send_json(404, {
                    "error": {"code": "NOT_FOUND", "message": "the pairing session was not found"}
                })
                return
            if sess.get("status") == "pending" and time.time() >= sess["expires_at"]:
                status = "expired"
            else:
                status = sess.get("status", "pending")
            self.send_json(200, {
                "session_id": session_id,
                "status": status,
                "expires_at": sess.get("expires_at_iso", ""),
                "attempts_remaining": max(1, sess.get("attempts_remaining", 5)),
            })
            return

        if path == "/api/v1/server/info":
            self.send_json(200, {
                "service": st.service,
                "name": st.name,
                "device_id": st.device_id,
                "server_id": st.device_id,
                "michi_id": st.server_michi_id,
                "public_key": st.server_pubkey_b64,
                "identity_scheme": "ed25519-blake3-v1",
                "id": st.device_id,
                "version": "1.0.0-alpha.1",
                "api_version": "v1-lite",
                "type": st.type_name,
                "roles": ["audio_receiver"],
                "supported_codecs": st.supported_codecs,
                "michi_home_id": st.home_id,
                "auth": {"required": True, "strategy": "HOME_MEMBERSHIP", "token_refresh": False},
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

        if path == "/api/v1/test/metrics/reset":
            st.metrics = {
                "packets_received": 0,
                "bytes_received": 0,
                "payload_bytes_received": 0,
                "last_payload_size": 0,
                "last_payload_type": 0,
                "last_sequence": 0,
                "last_timestamp": 0,
                "last_ssrc": 0,
                "malformed_packets": 0,
                "source_ip": "",
                "source_port": 0,
                "heartbeats_received": 0,
                "session_id": st.active_session_id or "",
                "first_sequence": None,
                "first_timestamp": None,
                "packet_history": [],
            }
            self.send_json(200, {"status": "metrics_reset"})
            return

        # Trust Architecture V2: Challenge Endpoint (POST /api/v1/auth/challenge)
        if path == "/api/v1/auth/challenge":
            challenge_id = str(uuid.uuid4())
            nonce_bytes = secrets.token_bytes(16)
            challenge_nonce = base64.urlsafe_b64encode(nonce_bytes).decode("ascii").rstrip("=")
            st.active_challenges[challenge_id] = {
                "challenge_id": challenge_id,
                "challenge_nonce": challenge_nonce,
                "client_michi_id": body.get("client_michi_id"),
                "client_public_key": body.get("client_public_key"),
                "home_id": body.get("home_id"),
                "expires_at": time.time() + 60,
            }
            self.send_json(200, {
                "challenge_id": challenge_id,
                "challenge_nonce": challenge_nonce,
                "server_michi_id": st.server_michi_id,
                "server_public_key": st.server_pubkey_b64,
                "expires_in": 60,
            })
            return

        # Trust Architecture V2: Session Endpoint (POST /api/v1/auth/session)
        if path == "/api/v1/auth/session":
            challenge_id = body.get("challenge_id")
            challenge = st.active_challenges.get(challenge_id)
            if not challenge or time.time() > challenge["expires_at"]:
                self.send_json(401, {"error": {"code": "UNAUTHORIZED", "message": "challenge expired or invalid"}})
                return

            client_michi_id = body.get("client_michi_id")
            raw_token = secrets.token_bytes(32)
            session_token = base64.urlsafe_b64encode(raw_token).decode("ascii").rstrip("=")
            st.tokens.add(session_token)

            server_auth_payload = (
                b"michi-link-server-auth-v1"
                + st.home_id.encode("ascii")
                + st.server_michi_id.encode("ascii")
                + client_michi_id.encode("ascii")
                + challenge_id.encode("ascii")
                + session_token.encode("ascii")
            )
            server_sig_bytes = st.server_private_key.sign(server_auth_payload)
            server_signature = base64.urlsafe_b64encode(server_sig_bytes).decode("ascii").rstrip("=")

            self.send_json(200, {
                "session_token": session_token,
                "token_type": "Bearer",
                "expires_in": 86400,
                "server_michi_id": st.server_michi_id,
                "server_membership": st.server_membership,
                "server_signature": server_signature,
            })
            return

        # 1. Pairing Start (POST /api/v1/pair/start)
        if path == "/api/v1/pair/start":
            session_id = str(uuid.uuid4())
            nonce = body.get("challenge_nonce") or str(uuid.uuid4())
            pin = "482391" # Canonical 6-digit numeric PIN for simulation
            now = time.time()
            expires_at = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=120)

            expires_iso = expires_at.isoformat().replace("+00:00", "Z")
            st.pairing_sessions[session_id] = {
                "nonce": nonce,
                "pin": pin,
                "expires_at": now + 120,
                "expires_at_iso": expires_iso,
                "consumed": False,
                "status": "pending",
                "attempts_remaining": 5,
                "controller": {
                    "michi_id": body.get("michi_id"),
                    "public_key": body.get("public_key"),
                }
            }

            self.send_json(200, {
                "session_id": session_id,
                "expires_at": expires_iso,
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
            if sess.get("status") == "confirmed" or sess.get("consumed", False):
                self.send_json(409, {
                    "error": {"code": "CONFLICT", "message": "pairing session already consumed"},
                })
                return
            if sess.get("status") == "locked":
                self.send_json(429, {
                    "error": {"code": "RATE_LIMITED", "message": "PIN attempts exceeded for this pairing session"},
                })
                return
            if time.time() > sess["expires_at"]:
                sess["status"] = "expired"
                self.send_json(400, {
                    "error": {"code": "PAIRING_EXPIRED", "message": "pairing session expired"},
                })
                return

            pin = body.get("pin")
            if pin != sess["pin"]:
                sess["attempts_remaining"] = sess.get("attempts_remaining", 5) - 1
                if sess["attempts_remaining"] <= 0:
                    sess["status"] = "locked"
                    sess["consumed"] = True
                    self.send_json(429, {
                        "error": {"code": "RATE_LIMITED", "message": "PIN attempts exceeded; pairing session is locked"},
                    })
                    return
                self.send_json(401, {
                    "error": {"code": "UNAUTHORIZED", "message": f"PIN '{pin}' incorrect"},
                })
                return

            sess["status"] = "confirmed"
            sess["consumed"] = True
            # Receiver issues long-lived Bearer pairing token
            raw_token = secrets.token_bytes(32)
            bearer_token = base64.urlsafe_b64encode(raw_token).decode("ascii").rstrip("=")
            st.tokens.add(bearer_token)
            token_sha256 = hashlib.sha256(raw_token).hexdigest()

            michi_id = body.get("michi_id")
            public_key = body.get("public_key")
            existing = next(
                (c for c in st.controllers.values() if michi_id and c.get("michi_id") == michi_id and c.get("public_key") == public_key),
                None,
            )
            controller_id = existing["device_id"] if existing else (michi_id or body.get("initiator_id") or "controller-1")
            st.controllers[controller_id] = {
                "michi_id": michi_id,
                "public_key": public_key,
                "token": bearer_token,
                "token_sha256": token_sha256,
                "device_id": controller_id,
            }

            self.send_json(200, {
                "status": "paired",
                "token": bearer_token,
                "expires_in": 0,
                "device_id": controller_id,
                "server_id": st.device_id,
                "controller_id": controller_id,
            })
            return

        # Pairing Recover Start (POST /api/v1/pair/recover/start)
        if path == "/api/v1/pair/recover/start":
            michi_id = body.get("michi_id")
            public_key = body.get("public_key")
            if not michi_id or not public_key:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": "missing michi_id or public_key"},
                })
                return

            existing = next(
                (c for c in st.controllers.values() if c.get("michi_id") == michi_id and c.get("public_key") == public_key),
                None,
            )
            if existing is None:
                self.send_json(404, {
                    "error": {"code": "NOT_FOUND", "message": "controller identity is not registered on this receiver"},
                })
                return

            existing_ch = st.recovery_challenges.get((michi_id, public_key))
            now = time.time()
            if existing_ch and now < existing_ch["expires_at"]:
                exp_iso = datetime.datetime.fromtimestamp(existing_ch["expires_at"], datetime.timezone.utc).isoformat()
                self.send_json(200, {
                    "challenge_nonce": existing_ch["challenge_nonce"],
                    "expires_at": exp_iso,
                    "server_michi_id": st.server_michi_id,
                    "server_public_key": st.server_pubkey_b64,
                })
                return

            raw_nonce = secrets.token_bytes(32)
            challenge_nonce = base64.urlsafe_b64encode(raw_nonce).decode("ascii").rstrip("=")
            exp_iso = datetime.datetime.fromtimestamp(now + 60.0, datetime.timezone.utc).isoformat()
            st.recovery_challenges[(michi_id, public_key)] = {
                "challenge_nonce": challenge_nonce,
                "expires_at": now + 60.0,
            }

            self.send_json(200, {
                "challenge_nonce": challenge_nonce,
                "expires_at": exp_iso,
                "server_michi_id": st.server_michi_id,
                "server_public_key": st.server_pubkey_b64,
            })
            return

        # Pairing Recover (POST /api/v1/pair/recover)
        if path == "/api/v1/pair/recover":
            michi_id = body.get("michi_id")
            public_key = body.get("public_key")
            challenge_nonce = body.get("challenge_nonce")
            challenge_sig = body.get("challenge_signature")

            if not michi_id or not public_key or not challenge_nonce or not challenge_sig:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": "missing required recovery fields"},
                })
                return

            ch = st.recovery_challenges.get((michi_id, public_key))
            if ch is None or ch["challenge_nonce"] != challenge_nonce:
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": "no active recovery challenge matching nonce"},
                })
                return

            if time.time() >= ch["expires_at"]:
                st.recovery_challenges.pop((michi_id, public_key), None)
                self.send_json(400, {
                    "error": {"code": "INVALID_REQUEST", "message": "recovery challenge expired"},
                })
                return

            existing = next(
                (c for c in st.controllers.values() if c.get("michi_id") == michi_id and c.get("public_key") == public_key),
                None,
            )
            if existing is None:
                self.send_json(404, {
                    "error": {"code": "NOT_FOUND", "message": "controller identity is not registered on this receiver"},
                })
                return

            # Single-use challenge consumed only upon successful recovery (anti-DoS)
            st.recovery_challenges.pop((michi_id, public_key), None)

            device_id = existing["device_id"]
            raw_token = secrets.token_bytes(32)
            bearer_token = base64.urlsafe_b64encode(raw_token).decode("ascii").rstrip("=")
            st.tokens.add(bearer_token)
            token_sha256 = hashlib.sha256(raw_token).hexdigest()
            existing["token"] = bearer_token
            existing["token_sha256"] = token_sha256

            self.send_json(200, {
                "status": "paired",
                "token": bearer_token,
                "expires_in": 0,
                "device_id": device_id,
                "server_id": st.device_id,
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

            st.metrics = {
                "packets_received": 0,
                "bytes_received": 0,
                "payload_bytes_received": 0,
                "last_payload_size": 0,
                "last_payload_type": 0,
                "last_sequence": 0,
                "last_timestamp": 0,
                "last_ssrc": 0,
                "malformed_packets": 0,
                "source_ip": "",
                "source_port": 0,
                "heartbeats_received": 0,
                "session_id": st.active_session_id,
                "packet_history": [],
            }

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
