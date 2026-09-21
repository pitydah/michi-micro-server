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


def test_soak_emits_failure_artifact_on_invalid_pid(mock_server):
    """soak_test.py must emit a valid failure JSON artifact even if target PID does not exist."""
    report_file = tempfile.mktemp(suffix=".json")
    try:
        # Pass a nonexistent PID (e.g. 9999999)
        res = subprocess.run(
            [
                sys.executable,
                SOAK_SCRIPT,
                "--url", mock_server,
                "--pid", "9999999",
                "--duration-seconds", "5",
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, "Expected non-zero exit on invalid PID"
        assert os.path.exists(report_file), "Expected failure report JSON to be created on early exit"

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "FAIL"
        assert data["exit_code"] == 1
        assert any("SERVER_PROCESS_NOT_RUNNING" in v for v in data["violations"])
        assert "SERVER_PROCESS_NOT_RUNNING" in data["violation_codes"]

        # Semantics of unobservable metrics: must be None (JSON null), never manufactured 0 / 0.0
        assert data["initial_rss_mb"] is None
        assert data["baseline_rss_mb"] is None
        assert data["final_rss_mb"] is None
        assert data["peak_rss_mb"] is None
        assert data["total_rss_drift_mb"] is None
        assert data["post_warmup_rss_drift_mb"] is None
        assert data["rss_slope_mb_per_hour"] is None
        assert data["initial_fds"] is None
        assert data["baseline_fds"] is None
        assert data["final_fds"] is None
        assert data["peak_fds"] is None
        assert data["fd_drift"] is None
        assert data["initial_threads"] is None
        assert data["baseline_threads"] is None
        assert data["final_threads"] is None
        assert data["thread_drift"] is None
        assert data["peak_wal_bytes"] is None
        assert data["child_processes"] is None
    finally:
        if os.path.exists(report_file):
            os.remove(report_file)


def test_soak_fails_closed_on_invalid_long_soak_duration():
    """soak_test.py must emit a FAIL report artifact if LONG_SOAK duration < 24h."""
    report_file = tempfile.mktemp(suffix=".json")
    try:
        res = subprocess.run(
            [
                sys.executable,
                SOAK_SCRIPT,
                "--pid", "1",
                "--evidence-class", "LONG_SOAK",
                "--duration-seconds", "100",  # < 86400s
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, "Expected non-zero exit for LONG_SOAK < 24h"
        assert os.path.exists(report_file), "Expected failure report JSON artifact for invalid LONG_SOAK duration"

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "FAIL"
        assert data["exit_code"] == 1
        assert any("INVALID_LONG_SOAK_DURATION" in v for v in data["violations"])
        assert "INVALID_LONG_SOAK_DURATION" in data["violation_codes"]
    finally:
        if os.path.exists(report_file):
            os.remove(report_file)


def test_soak_passes_warmup_jump_followed_by_stability(mock_server):
    """A large memory growth during warm-up followed by stability must PASS without false positive."""
    script = """
import time
# Warm-up is 2.0 seconds. Allocate 30MB during first 1.0s (warmup jump)
arrays = []
for _ in range(10):
    arrays.append(bytearray(3 * 1024 * 1024))
    time.sleep(0.1)
# Now settled and stable for remaining observation window
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
                "--warmup-seconds", "2.0",
                "--sample-interval", "0.5",
                "--max-rss-mb", "200.0",
                "--max-post-warmup-drift-mb", "8.0",
                "--max-rss-slope-mb-per-hour", "50.0",
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode == 0, f"Expected PASS for warm-up jump followed by stability:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}"

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "PASS"
        assert len(data["violations"]) == 0
        # Post-warmup growth should be negligible (< 3.0MB) even though total growth from t=0 was ~30MB
        assert data["post_warmup_rss_drift_mb"] < 3.0
        assert data["total_rss_drift_mb"] >= 20.0
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)


def test_validate_duration_coverage_pure():
    """Mathematical validation of temporal duration coverage for LONG_SOAK."""
    sys.path.insert(0, os.path.join(ROOT_DIR, "scripts"))
    from soak_test import validate_duration_coverage

    # 1. LONG_SOAK with requested < 24h fails closed
    ok, err = validate_duration_coverage("LONG_SOAK", 3600, 3600)
    assert not ok
    assert "INVALID_LONG_SOAK_DURATION" in err

    # 2. LONG_SOAK with requested 24h but actual elapsed < 99.9% fails closed
    ok, err = validate_duration_coverage("LONG_SOAK", 86400, 80000)
    assert not ok
    assert "LONG_SOAK_DURATION_INCOMPLETE" in err

    # 3. LONG_SOAK with requested 24h and actual elapsed >= 99.9% passes (e.g. 86350s >= 86313.6s)
    ok, err = validate_duration_coverage("LONG_SOAK", 86400, 86350)
    assert ok
    assert err is None

    # 4. Other evidence classes pass without 24h requirement
    ok, err = validate_duration_coverage("INTEGRATION_REAL", 90, 90)
    assert ok
    assert err is None


def test_soak_fails_on_fd_leak(mock_server):
    """soak_test.py must detect file descriptor leak and fail with FD_LEAK_DETECTED."""
    script = """
import time, os
# Warm-up is 1.5 seconds.
time.sleep(1.8)
# Post-warmup: open 20 pipe pairs and keep them open (40 FDs)
pipes = []
for _ in range(20):
    pipes.append(os.pipe())
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
                "--duration-seconds", "5",
                "--warmup-seconds", "1.5",
                "--sample-interval", "0.5",
                "--max-fd-drift", "5",
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, f"Expected failure on FD leak:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}"
        assert os.path.exists(report_file), "Expected report JSON to exist"

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "FAIL"
        assert "FD_LEAK_DETECTED" in data["violation_codes"]
        assert any("FD_LEAK_DETECTED" in v for v in data["violations"])
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)


def test_soak_fails_on_thread_leak(mock_server):
    """soak_test.py must detect thread leak post-warmup and fail with THREAD_LEAK_DETECTED."""
    script = """
import time, threading
# Warm-up is 1.5 seconds.
time.sleep(1.8)
# Post-warmup: spawn 10 worker threads and keep them alive
threads = []
for _ in range(10):
    t = threading.Thread(target=lambda: time.sleep(15), daemon=True)
    t.start()
    threads.append(t)
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
                "--duration-seconds", "5",
                "--warmup-seconds", "1.5",
                "--sample-interval", "0.5",
                "--max-thread-drift", "3",
                "--report", report_file,
            ],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, f"Expected failure on thread leak:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}"
        assert os.path.exists(report_file), "Expected report JSON to exist"

        with open(report_file, "r") as f:
            data = json.load(f)
        assert data["status"] == "FAIL"
        assert "THREAD_LEAK_DETECTED" in data["violation_codes"]
        assert any("THREAD_LEAK_DETECTED" in v for v in data["violations"])
    finally:
        proc.kill()
        if os.path.exists(report_file):
            os.remove(report_file)


