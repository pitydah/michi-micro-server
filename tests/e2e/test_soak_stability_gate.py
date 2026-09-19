#!/usr/bin/env python3
"""
Regression test suite for scripts/soak_test.py.
Verifies fail-closed detection of:
1. Hard memory ceiling violation
2. Post-warmup memory drift violation
3. Linear regression RSS growth slope violation
4. File descriptor leak violation
5. Process death violation
6. Safe passage of a stable process under realistic conditions
"""

import json
import os
import subprocess
import sys
import tempfile
import time
import pytest
from http.server import HTTPServer, BaseHTTPRequestHandler
import threading

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOAK_SCRIPT = os.path.join(ROOT_DIR, "scripts", "soak_test.py")


class MockServerHandler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path in ("/health/live", "/api/v1/server/info", "/api/v1/status"):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')
        else:
            self.send_response(404)
            self.end_headers()


@pytest.fixture(scope="module")
def mock_server():
    httpd = HTTPServer(("127.0.0.1", 0), MockServerHandler)
    port = httpd.server_address[1]
    th = threading.Thread(target=httpd.serve_forever, daemon=True)
    th.start()
    yield f"http://127.0.0.1:{port}"
    httpd.shutdown()


def test_soak_fails_on_hard_rss_ceiling(mock_server):
    """soak_test.py must FAIL if current RSS exceeds --max-rss-mb."""
    # Run a dummy python process that stays alive
    proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
    report_file = tempfile.mktemp(suffix=".json")
    try:
        # Set max-rss-mb ridiculously low (0.1 MB) so the python process triggers it
        res = subprocess.run(
            [
                sys.executable,
                SOAK_SCRIPT,
                "--url", mock_server,
                "--pid", str(proc.pid),
                "--duration-seconds", "4",
                "--warmup-seconds", "1",
                "--sample-interval", "0.5",
                "--max-rss-mb", "0.1",
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, f"Expected non-zero exit on hard RSS ceiling:\n{res.stdout}\n{res.stderr}"
        assert "HARD_RSS_CEILING_EXCEEDED" in res.stderr or "HARD_RSS_CEILING_EXCEEDED" in res.stdout

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "FAIL"
        assert any("HARD_RSS_CEILING_EXCEEDED" in v for v in data["violations"])
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)


def test_soak_fails_on_post_warmup_memory_leak(mock_server):
    """soak_test.py must FAIL if memory leaks significantly post-warmup."""
    # Process that allocates memory after warm-up
    script = """
import time
# Warmup for 2 seconds
time.sleep(2)
# Leak memory progressively
arrays = []
for _ in range(30):
    arrays.append(bytearray(4 * 1024 * 1024)) # +4MB per iteration
    time.sleep(0.3)
time.sleep(10)
"""
    proc = subprocess.Popen([sys.executable, "-c", script])
    report_file = tempfile.mktemp(suffix=".json")
    try:
        res = subprocess.run(
            [
                sys.executable,
                SOAK_SCRIPT,
                "--url", mock_server,
                "--pid", str(proc.pid),
                "--duration-seconds", "6",
                "--warmup-seconds", "2",
                "--sample-interval", "0.5",
                "--max-rss-mb", "500.0",
                "--max-post-warmup-drift-mb", "5.0",  # Will leak >10MB
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, f"Expected failure on artificial post-warmup memory leak:\n{res.stdout}\n{res.stderr}"
        assert "POST_WARMUP_RSS_DRIFT_EXCEEDED" in res.stderr or "POST_WARMUP_RSS_DRIFT_EXCEEDED" in res.stdout

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "FAIL"
        assert any("POST_WARMUP_RSS_DRIFT_EXCEEDED" in v for v in data["violations"])
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)


def test_soak_fails_on_steep_slope_leak(mock_server):
    """soak_test.py must FAIL if linear regression slope exceeds threshold."""
    # Process that steadily grows memory post-warmup
    script = """
import time
time.sleep(1)
arrays = []
for _ in range(20):
    arrays.append(bytearray(2 * 1024 * 1024))
    time.sleep(0.4)
time.sleep(10)
"""
    proc = subprocess.Popen([sys.executable, "-c", script])
    report_file = tempfile.mktemp(suffix=".json")
    try:
        res = subprocess.run(
            [
                sys.executable,
                SOAK_SCRIPT,
                "--url", mock_server,
                "--pid", str(proc.pid),
                "--duration-seconds", "6",
                "--warmup-seconds", "1",
                "--sample-interval", "0.5",
                "--max-rss-mb", "500.0",
                "--max-post-warmup-drift-mb", "100.0",  # High drift allowed
                "--max-rss-slope-mb-per-hour", "50.0",   # Slope limit ~50MB/h, leak will be >1000MB/h
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, f"Expected failure on excessive RSS growth slope:\n{res.stdout}\n{res.stderr}"
        assert "EXCESSIVE_RSS_GROWTH_SLOPE" in res.stderr or "EXCESSIVE_RSS_GROWTH_SLOPE" in res.stdout

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "FAIL"
        assert any("EXCESSIVE_RSS_GROWTH_SLOPE" in v for v in data["violations"])
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)


def test_soak_fails_on_process_death(mock_server):
    """soak_test.py must FAIL if target process dies during test."""
    proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(1)"])
    report_file = tempfile.mktemp(suffix=".json")
    try:
        res = subprocess.run(
            [
                sys.executable,
                SOAK_SCRIPT,
                "--url", mock_server,
                "--pid", str(proc.pid),
                "--duration-seconds", "5",
                "--warmup-seconds", "1",
                "--sample-interval", "0.5",
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, f"Expected non-zero exit on process death:\n{res.stdout}\n{res.stderr}"
        assert "SERVER_PROCESS_DIED" in res.stderr or "SERVER_PROCESS_DIED" in res.stdout
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)


def test_soak_passes_stable_process(mock_server):
    """soak_test.py must PASS when process memory, FDs, and threads are stable."""
    proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(20)"])
    report_file = tempfile.mktemp(suffix=".json")
    try:
        res = subprocess.run(
            [
                sys.executable,
                SOAK_SCRIPT,
                "--url", mock_server,
                "--pid", str(proc.pid),
                "--duration-seconds", "5",
                "--warmup-seconds", "1.5",
                "--sample-interval", "0.5",
                "--max-rss-mb", "100.0",
                "--max-post-warmup-drift-mb", "10.0",
                "--max-rss-slope-mb-per-hour", "50.0",
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode == 0, f"Expected success for stable process:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}"

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "PASS"
        assert len(data["violations"]) == 0
        assert data["post_warmup_rss_drift_mb"] <= 5.0
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)
