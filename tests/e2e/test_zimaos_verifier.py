#!/usr/bin/env python3
"""
Regression test suite for scripts/verify_zimaos_install.py.
Verifies fail-closed behavior on unreachable server, version mismatch,
and platform mismatch.
"""

import os
import subprocess
import sys
import json
import pytest

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SCRIPT_PATH = os.path.join(ROOT_DIR, "scripts", "verify_zimaos_install.py")


def test_local_dist_verification_succeeds():
    """Local dist layout must pass verification."""
    res = subprocess.run(
        [sys.executable, SCRIPT_PATH, "--dist-dir", os.path.join(ROOT_DIR, "dist")],
        capture_output=True,
        text=True,
    )
    assert res.returncode == 0, f"Local dist verification failed:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}"


def test_verify_fails_on_unreachable_server():
    """Verification must exit 1 (fail closed) when server is unreachable."""
    res = subprocess.run(
        [
            sys.executable,
            SCRIPT_PATH,
            "--server-url",
            "http://127.0.0.1:59999",
        ],
        capture_output=True,
        text=True,
    )
    assert res.returncode != 0, f"Expected non-zero exit code on unreachable server, got {res.returncode}"
    assert "Verification FAILED" in res.stdout or "Verification FAILED" in res.stderr


def test_verify_with_mock_server():
    """Test verify_running_server against mock HTTP server for pass and fail-closed cases."""
    from http.server import HTTPServer, BaseHTTPRequestHandler
    import threading

    server_data = {
        "version": "1.0.0-rc.2",
        "commit": "abcd1234abcd",
        "deployment_platform": "zimaos"
    }

    class MockHandler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            if self.path == "/health/live":
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.end_headers()
                self.wfile.write(b"OK")
            elif self.path in ("/", "/sw.js"):
                self.send_response(200)
                self.send_header("Cache-Control", "no-cache, no-store, must-revalidate")
                self.end_headers()
                self.wfile.write(b"// mock")
            elif self.path == "/api/v1/server/info":
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                resp = {
                    "service": "michi-micro-server",
                    "api_version": "v1",
                    "version": server_data["version"],
                    "commit": server_data["commit"],
                    "deployment_platform": server_data["deployment_platform"]
                }
                self.wfile.write(json.dumps(resp).encode())
            elif self.path == "/api/v1/update/status":
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                resp = {
                    "status": "up_to_date",
                    "current_version": server_data["version"],
                    "commit": server_data["commit"],
                    "deployment_platform": server_data["deployment_platform"]
                }
                self.wfile.write(json.dumps(resp).encode())
            else:
                self.send_response(404)
                self.end_headers()

    httpd = HTTPServer(("127.0.0.1", 0), MockHandler)
    port = httpd.server_address[1]
    th = threading.Thread(target=httpd.serve_forever, daemon=True)
    th.start()
    server_url = f"http://127.0.0.1:{port}"

    try:
        # 1. Matching case passes
        res = subprocess.run(
            [
                sys.executable,
                SCRIPT_PATH,
                "--server-url", server_url,
                "--expected-version", "1.0.0-rc.2",
                "--expected-commit", "abcd1234abcd",
                "--expected-platform", "zimaos",
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode == 0, f"Expected success with matching params:\n{res.stdout}\n{res.stderr}"

        # 2. Version mismatch fails closed
        res_v = subprocess.run(
            [
                sys.executable,
                SCRIPT_PATH,
                "--server-url", server_url,
                "--expected-version", "9.9.9",
            ],
            capture_output=True,
            text=True,
        )
        assert res_v.returncode != 0, "Expected failure on version mismatch"

        # 3. Commit mismatch fails closed
        res_c = subprocess.run(
            [
                sys.executable,
                SCRIPT_PATH,
                "--server-url", server_url,
                "--expected-commit", "wrongcommit",
            ],
            capture_output=True,
            text=True,
        )
        assert res_c.returncode != 0, "Expected failure on commit mismatch"

        # 4. Platform mismatch fails closed
        res_p = subprocess.run(
            [
                sys.executable,
                SCRIPT_PATH,
                "--server-url", server_url,
                "--expected-platform", "docker",
            ],
            capture_output=True,
            text=True,
        )
        assert res_p.returncode != 0, "Expected failure on platform mismatch"

    finally:
        httpd.shutdown()
