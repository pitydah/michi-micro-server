#!/usr/bin/env python3
"""
Autonomous Snapserver JSON-RPC 2.0 Mock Server.
Emulates a real Snapcast server with multiple clients and groups for integration testing.

Usage:
  python3 scripts/snapserver_mock.py --port 1780
"""

import argparse
import json
import time
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn

class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True

class SnapserverState:
    def __init__(self, port=1780):
        self.port = port
        self.version = "0.29.0"
        self.offline = False
        
        # Initialize default groups and clients
        self.groups = [
            {
                "id": "group-living-room",
                "name": "Living Room",
                "muted": False,
                "volume": {"percent": 80, "muted": False},
                "stream_id": "default",
                "clients": [
                    {
                        "id": "client-speaker-lr-1",
                        "name": "Speaker Left",
                        "host": {"ip": "192.168.1.101", "name": "lr-spk-1"},
                        "connected": True,
                        "config": {"volume": {"percent": 80, "muted": False}, "latency": 0, "name": "Speaker Left"},
                        "lastSeen": {"sec": int(time.time()), "usec": 0}
                    },
                    {
                        "id": "client-speaker-lr-2",
                        "name": "Speaker Right",
                        "host": {"ip": "192.168.1.102", "name": "lr-spk-2"},
                        "connected": True,
                        "config": {"volume": {"percent": 80, "muted": False}, "latency": 0, "name": "Speaker Right"},
                        "lastSeen": {"sec": int(time.time()), "usec": 0}
                    }
                ]
            },
            {
                "id": "group-kitchen",
                "name": "Kitchen",
                "muted": False,
                "volume": {"percent": 65, "muted": False},
                "stream_id": "default",
                "clients": [
                    {
                        "id": "client-kitchen-1",
                        "name": "Kitchen Pod",
                        "host": {"ip": "192.168.1.103", "name": "kitchen-spk"},
                        "connected": True,
                        "config": {"volume": {"percent": 65, "muted": False}, "latency": 50, "name": "Kitchen Pod"},
                        "lastSeen": {"sec": int(time.time()), "usec": 0}
                    }
                ]
            }
        ]

class SnapserverHandler(BaseHTTPRequestHandler):
    state: SnapserverState = None

    def log_message(self, format, *args):
        pass  # Quiet logging for test runner

    def send_json(self, status, payload):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        # Health check
        if self.path in ("/health", "/"):
            self.send_json(200, {"status": "ok", "service": "snapserver-mock"})
            return
        self.send_json(404, {"error": "not found"})

    def do_POST(self):
        st = self.state
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length).decode("utf-8") if length > 0 else "{}"
        try:
            body = json.loads(raw)
        except Exception:
            body = {}

        # Admin controls for fault injection & client simulation (immune to offline)
        if self.path == "/api/admin/client/disconnect":
            cid = body.get("client_id")
            for g in st.groups:
                for c in g["clients"]:
                    if c["id"] == cid:
                        c["connected"] = False
            self.send_json(200, {"status": "client_disconnected", "client_id": cid})
            return

        if self.path == "/api/admin/client/reconnect":
            cid = body.get("client_id")
            for g in st.groups:
                for c in g["clients"]:
                    if c["id"] == cid:
                        c["connected"] = True
            self.send_json(200, {"status": "client_reconnected", "client_id": cid})
            return

        if self.path == "/api/admin/offline":
            st.offline = body.get("offline", True)
            self.send_json(200, {"status": "offline_updated", "offline": st.offline})
            return

        if st.offline:
            self.send_json(503, {"error": "snapserver offline (mock fault)"})
            return

        # Handle Snapcast JSON-RPC methods
        req_id = body.get("id", 1)
        method = body.get("method", "")
        params = body.get("params", {})

        if method == "Server.GetStatus":
            self.send_json(200, {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "server": {
                        "version": st.version,
                        "host": {"ip": "127.0.0.1", "name": "snapserver-mock"},
                        "groups": st.groups,
                        "streams": [
                            {"id": "default", "status": "playing", "uri": "pipe:///tmp/snapfifo"}
                        ]
                    }
                }
            })
            return

        if method == "Group.SetVolume":
            gid = params.get("id")
            vol_obj = params.get("volume", {})
            pct = vol_obj.get("percent", 100)
            muted = vol_obj.get("muted", False)
            for g in st.groups:
                if g["id"] == gid:
                    g["volume"]["percent"] = pct
                    g["volume"]["muted"] = muted
                    g["muted"] = muted
            self.send_json(200, {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"volume": {"percent": pct, "muted": muted}}
            })
            return

        if method == "Group.SetMute":
            gid = params.get("id")
            mute = params.get("mute", False)
            for g in st.groups:
                if g["id"] == gid:
                    g["muted"] = mute
                    g["volume"]["muted"] = mute
            self.send_json(200, {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"mute": mute}
            })
            return

        if method == "Client.SetVolume":
            cid = params.get("id")
            vol_obj = params.get("volume", {})
            for g in st.groups:
                for c in g["clients"]:
                    if c["id"] == cid:
                        c["config"]["volume"] = vol_obj
            self.send_json(200, {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"volume": vol_obj}
            })
            return

        self.send_json(200, {
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {"code": -32601, "message": f"Method '{method}' not found"}
        })

def run_server(port=1780, host="0.0.0.0"):
    state = SnapserverState(port=port)
    handler = type("ConfiguredSnapserverHandler", (SnapserverHandler,), {"state": state})
    server = ThreadedHTTPServer((host, port), handler)
    print(f"Snapserver Mock running on http://{host}:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

def main():
    parser = argparse.ArgumentParser(description="Snapserver Mock")
    parser.add_argument("--port", type=int, default=1780)
    parser.add_argument("--host", default="0.0.0.0")
    args = parser.parse_args()
    run_server(args.port, args.host)

if __name__ == "__main__":
    main()
