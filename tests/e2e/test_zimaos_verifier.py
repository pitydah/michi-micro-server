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
import hashlib
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


def test_public_store_compose_tag_integrity():
    """Structural test asserting public compose references a tag maintained by publisher / product truth."""
    import yaml
    import tomllib

    with open(os.path.join(ROOT_DIR, "Cargo.toml"), "rb") as f:
        cargo = tomllib.load(f)
    product_version = cargo["workspace"]["package"]["version"]

    allowed_tags = {"r3.1-zima", product_version}
    store_composes = [
        os.path.join(ROOT_DIR, "zimaos-store", "Apps", "MichiMicroServer", "docker-compose.yml"),
        os.path.join(ROOT_DIR, "dist", "apps", "io.michi.micro-server", "docker-compose.yml"),
    ]

    for comp_path in store_composes:
        assert os.path.exists(comp_path), f"Missing store compose: {comp_path}"
        with open(comp_path, "r", encoding="utf-8") as f:
            data = yaml.safe_load(f)
        services = data.get("services", {})
        svc = services.get("michi-micro-server")
        assert svc, f"Missing service 'michi-micro-server' in {comp_path}"
        img = svc.get("image", "")
        assert img.startswith("ghcr.io/pitydah/michi-micro-server:"), f"Invalid repo prefix in {img}"
        tag = img.split(":", 1)[1]
        assert tag in allowed_tags, f"Image tag '{tag}' in {comp_path} is not in publisher maintained tags {allowed_tags}"


def test_verify_remote_store_adversarial_suite():
    """Test verify_remote_store fail-closed behavior on 7 adversarial conditions."""
    from http.server import HTTPServer, BaseHTTPRequestHandler
    import threading
    import copy

    default_store = {
        "version": 2,
        "store_id": "io.michi.store",
        "name": {"en_US": "Michi Official App Store"},
        "apps": [
            {
                "id": "io.michi.micro-server",
                "version": "1.0.0-rc.2",
            }
        ]
    }

    default_compose = """
name: michi-micro-server
services:
  michi-micro-server:
    image: ghcr.io/pitydah/michi-micro-server:r3.1-zima
    environment:
      - MICHI_DEPLOYMENT_PLATFORM=zimaos
      - MICHI_AUTH_PASSWORD=${MICHI_AUTH_PASSWORD:?Password required}
"""
    default_icon = b"<svg>mock icon</svg>"
    default_thumb = b"\x89PNG\r\n\x1a\nmock thumb"
    
    def calc_hash(comp_b, icon_b, thumb_b):
        h = hashlib.sha256()
        h.update(comp_b)
        h.update(icon_b)
        h.update(thumb_b)
        return h.hexdigest()

    default_meta = {
        "version": "1.0.0-rc.2",
        "content_hash": calc_hash(default_compose.strip().encode(), default_icon, default_thumb)
    }

    current_state = {
        "store_status": 200,
        "store_body": json.dumps(default_store).encode(),
        "compose_status": 200,
        "compose_body": default_compose.strip().encode(),
        "meta_status": 200,
        "meta_body": json.dumps(default_meta).encode(),
        "icon_status": 200,
        "icon_body": default_icon,
        "thumb_status": 200,
        "thumb_body": default_thumb,
    }

    class MockStoreHandler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            if self.path == "/dist/store.json":
                self.send_response(current_state["store_status"])
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(current_state["store_body"])
            elif self.path == "/dist/apps/io.michi.micro-server/docker-compose.yml":
                self.send_response(current_state["compose_status"])
                self.send_header("Content-Type", "text/yaml")
                self.end_headers()
                self.wfile.write(current_state["compose_body"])
            elif self.path == "/dist/apps/io.michi.micro-server/meta.json":
                self.send_response(current_state["meta_status"])
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(current_state["meta_body"])
            elif self.path == "/dist/apps/io.michi.micro-server/assets/icon.svg":
                self.send_response(current_state["icon_status"])
                self.send_header("Content-Type", "image/svg+xml")
                self.end_headers()
                self.wfile.write(current_state["icon_body"])
            elif self.path == "/dist/apps/io.michi.micro-server/assets/thumbnail.png":
                self.send_response(current_state["thumb_status"])
                self.send_header("Content-Type", "image/png")
                self.end_headers()
                self.wfile.write(current_state["thumb_body"])
            else:
                self.send_response(404)
                self.end_headers()

    httpd = HTTPServer(("127.0.0.1", 0), MockStoreHandler)
    port = httpd.server_address[1]
    th = threading.Thread(target=httpd.serve_forever, daemon=True)
    th.start()
    base_url = f"http://127.0.0.1:{port}"

    try:
        # Case 1: Baseline valid remote store passes
        res = subprocess.run(
            [sys.executable, SCRIPT_PATH, "--remote-url", base_url],
            capture_output=True,
            text=True,
        )
        assert res.returncode == 0, f"Expected valid remote store to pass:\n{res.stdout}\n{res.stderr}"

        # Case 2: Malformed JSON in store.json fails
        current_state["store_body"] = b"not-valid-json{{"
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected malformed JSON in store.json to fail"
        assert "malformed JSON" in res.stdout or "malformed JSON" in res.stderr

        # Case 3: Wrong store_id in store.json fails
        wrong_store = copy.deepcopy(default_store)
        wrong_store["store_id"] = "com.untrusted.store"
        current_state["store_body"] = json.dumps(wrong_store).encode()
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected wrong store_id to fail"
        assert "store_id must be 'io.michi.store'" in res.stdout or "store_id must be 'io.michi.store'" in res.stderr

        # Case 4: Missing io.michi.micro-server in store.json fails
        no_app_store = copy.deepcopy(default_store)
        no_app_store["apps"] = [{"id": "other.app"}]
        current_state["store_body"] = json.dumps(no_app_store).encode()
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected missing app in store.json to fail"
        assert "missing app 'io.michi.micro-server'" in res.stdout or "missing app 'io.michi.micro-server'" in res.stderr

        # Case 5: Stale app version in store.json fails
        stale_store = copy.deepcopy(default_store)
        stale_store["apps"][0]["version"] = "0.9.0"
        current_state["store_body"] = json.dumps(stale_store).encode()
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected stale app version to fail"
        assert "does not match expected" in res.stdout or "does not match expected" in res.stderr

        # Reset store.json to valid
        current_state["store_body"] = json.dumps(default_store).encode()

        # Case 6: Malformed YAML in docker-compose.yml fails
        current_state["compose_body"] = b"services:\n  bad_yaml: [unclosed"
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected malformed YAML in compose to fail"
        assert "malformed YAML" in res.stdout or "malformed YAML" in res.stderr

        # Case 7: Wrong MICHI_DEPLOYMENT_PLATFORM in docker-compose.yml fails
        wrong_platform_compose = default_compose.replace("MICHI_DEPLOYMENT_PLATFORM=zimaos", "MICHI_DEPLOYMENT_PLATFORM=generic")
        current_state["compose_body"] = wrong_platform_compose.encode()
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected wrong deployment platform to fail"
        assert "must set MICHI_DEPLOYMENT_PLATFORM=zimaos" in res.stdout or "must set MICHI_DEPLOYMENT_PLATFORM=zimaos" in res.stderr

        # Case 8: Missing ':?' parameter expansion in MICHI_AUTH_PASSWORD fails
        insecure_compose = default_compose.replace("${MICHI_AUTH_PASSWORD:?Password required}", "${MICHI_AUTH_PASSWORD:-defaultpass}")
        current_state["compose_body"] = insecure_compose.encode()
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected missing :? password expression to fail"
        assert "fail-closed ':?' parameter expansion" in res.stdout or "fail-closed ':?' parameter expansion" in res.stderr

        # Reset compose
        current_state["compose_body"] = default_compose.strip().encode()

        # Case 9: Image mismatch when --expected-image specified fails
        res = subprocess.run(
            [sys.executable, SCRIPT_PATH, "--remote-url", base_url, "--expected-image", "ghcr.io/pitydah/michi-micro-server:9.9.9"],
            capture_output=True,
            text=True,
        )
        assert res.returncode != 0, "Expected image mismatch to fail"
        assert "does not match expected" in res.stdout or "does not match expected" in res.stderr

        # Case 10: Stale version in meta.json fails
        stale_meta = dict(default_meta, version="0.8.0")
        current_state["meta_body"] = json.dumps(stale_meta).encode()
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected stale meta version to fail"
        assert "does not match expected" in res.stdout or "does not match expected" in res.stderr

        # Case 11: Wrong content_hash in meta.json fails
        wrong_hash_meta = dict(default_meta, content_hash="0" * 64)
        current_state["meta_body"] = json.dumps(wrong_hash_meta).encode()
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected content_hash mismatch to fail"
        assert "content_hash mismatch" in res.stdout or "content_hash mismatch" in res.stderr

        # Reset meta.json
        current_state["meta_body"] = json.dumps(default_meta).encode()

        # Case 12: Changed icon fails content_hash
        current_state["icon_body"] = b"<svg>tampered icon</svg>"
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected tampered icon to fail hash check"
        assert "content_hash mismatch" in res.stdout or "content_hash mismatch" in res.stderr

        # Reset icon, Case 13: Changed thumbnail fails content_hash
        current_state["icon_body"] = default_icon
        current_state["thumb_body"] = b"tampered thumbnail"
        res = subprocess.run([sys.executable, SCRIPT_PATH, "--remote-url", base_url], capture_output=True, text=True)
        assert res.returncode != 0, "Expected tampered thumbnail to fail hash check"
        assert "content_hash mismatch" in res.stdout or "content_hash mismatch" in res.stderr

    finally:
        httpd.shutdown()


def test_verify_running_server_auth_suite():
    """Test verify_running_server authentication and fail-closed handling."""
    from http.server import HTTPServer, BaseHTTPRequestHandler
    import threading

    class MockAuthServer(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            if self.path == "/api/auth/login":
                content_len = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(content_len).decode()
                try:
                    payload = json.loads(body)
                except Exception:
                    self.send_response(400)
                    self.end_headers()
                    return

                if payload.get("username") == "admin" and payload.get("password") == "secret123":
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({"token": "valid_session_token_xyz"}).encode())
                else:
                    self.send_response(401)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({"status": "error", "message": "Invalid credentials"}).encode())
            else:
                self.send_response(404)
                self.end_headers()

        def do_GET(self):
            if self.path == "/health/live":
                self.send_response(200)
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
                self.wfile.write(json.dumps({
                    "service": "michi-micro-server",
                    "api_version": "v1",
                    "version": "1.0.0-rc.2",
                    "commit": "abcd1234abcd",
                    "deployment_platform": "zimaos"
                }).encode())
            elif self.path == "/api/v1/update/status":
                auth_header = self.headers.get("Authorization", "")
                if auth_header != "Bearer valid_session_token_xyz":
                    self.send_response(401)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({"status": "error", "message": "Unauthorized"}).encode())
                else:
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({
                        "status": "up_to_date",
                        "deployment_platform": "zimaos",
                        "commit": "abcd1234abcd"
                    }).encode())
            else:
                self.send_response(404)
                self.end_headers()

    httpd = HTTPServer(("127.0.0.1", 0), MockAuthServer)
    port = httpd.server_address[1]
    th = threading.Thread(target=httpd.serve_forever, daemon=True)
    th.start()
    server_url = f"http://127.0.0.1:{port}"

    try:
        # 1. Unauthenticated call when endpoint is protected returns 401 and fails closed
        res_no_auth = subprocess.run(
            [sys.executable, SCRIPT_PATH, "--server-url", server_url],
            capture_output=True,
            text=True,
        )
        assert res_no_auth.returncode != 0, "Expected failure when protected endpoint returns 401"
        assert "401 Unauthorized" in res_no_auth.stdout or "401 Unauthorized" in res_no_auth.stderr

        # 2. Correct username/password authenticates and passes
        res_creds = subprocess.run(
            [
                sys.executable,
                SCRIPT_PATH,
                "--server-url", server_url,
                "--username", "admin",
                "--password", "secret123",
                "--expected-version", "1.0.0-rc.2",
                "--expected-commit", "abcd1234abcd",
                "--expected-platform", "zimaos",
            ],
            capture_output=True,
            text=True,
        )
        assert res_creds.returncode == 0, f"Expected successful authenticated verification:\n{res_creds.stdout}\n{res_creds.stderr}"
        assert "Authenticated via /api/auth/login successfully" in res_creds.stdout

        # 3. Invalid credentials fails closed
        res_bad_creds = subprocess.run(
            [
                sys.executable,
                SCRIPT_PATH,
                "--server-url", server_url,
                "--username", "admin",
                "--password", "wrongpassword",
            ],
            capture_output=True,
            text=True,
        )
        assert res_bad_creds.returncode != 0, "Expected failure on bad credentials"
        assert "Authentication failed" in res_bad_creds.stdout or "Authentication failed" in res_bad_creds.stderr

        # 4. Direct bearer token passes
        res_token = subprocess.run(
            [
                sys.executable,
                SCRIPT_PATH,
                "--server-url", server_url,
                "--token", "valid_session_token_xyz",
                "--expected-platform", "zimaos",
            ],
            capture_output=True,
            text=True,
        )
        assert res_token.returncode == 0, f"Expected valid token to pass:\n{res_token.stdout}\n{res_token.stderr}"

    finally:
        httpd.shutdown()

