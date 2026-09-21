#!/usr/bin/env python3
"""
ZimaOS / CasaOS Package Delivery & Runtime Acceptance Verification

Validates that:
1. Store distribution files (dist/store.json, dist/index.json, dist/apps/...) are complete,
   structurally valid, and hash-consistent.
2. The docker-compose configuration specifies MICHI_DEPLOYMENT_PLATFORM=zimaos and correct ports/volumes.
3. If remote URL is provided, verifies that raw GitHub endpoints are publicly accessible and uncorrupted.
4. If running server URL is provided, verifies that /api/v1/settings and /api/v1/update/status report
   truthful platform identity ("zimaos") and that Cache-Control headers prevent stale WebUI caching.
"""

import os
import sys
import json
import argparse
import urllib.request
import urllib.error
import hashlib
import yaml

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import tomllib

def get_product_version():
    with open(os.path.join(ROOT_DIR, "Cargo.toml"), "rb") as f:
        cargo = tomllib.load(f)
    return cargo["workspace"]["package"]["version"]

def verify_local_dist(dist_dir, expected_image=None):
    print(f"[1/4] Verifying local store distribution layout at {dist_dir}...")
    errors = []

    store_json_path = os.path.join(dist_dir, "store.json")
    index_json_path = os.path.join(dist_dir, "index.json")
    app_dir = os.path.join(dist_dir, "apps", "io.michi.micro-server")
    compose_path = os.path.join(app_dir, "docker-compose.yml")
    meta_path = os.path.join(app_dir, "meta.json")
    icon_path = os.path.join(app_dir, "assets", "icon.svg")
    thumb_path = os.path.join(app_dir, "assets", "thumbnail.png")

    for p in [store_json_path, index_json_path, compose_path, meta_path, icon_path, thumb_path]:
        if not os.path.exists(p) or os.path.getsize(p) == 0:
            errors.append(f"Missing or empty required file: {p}")

    if errors:
        for err in errors:
            print(f"  ❌ {err}")
        return False, errors

    # Check store.json
    with open(store_json_path, "r", encoding="utf-8") as f:
        store_data = json.load(f)
    if store_data.get("store_id") != "io.michi.store":
        errors.append(f"store.json store_id must be 'io.michi.store', got {store_data.get('store_id')}")

    # Check meta.json
    with open(meta_path, "r", encoding="utf-8") as f:
        meta_data = json.load(f)
    expected_version = get_product_version()
    if meta_data.get("version") != expected_version:
        errors.append(f"meta.json version {meta_data.get('version')} does not match Cargo.toml {expected_version}")

    # Verify content hash matches compose + assets
    sha256 = hashlib.sha256()
    with open(compose_path, "rb") as f:
        sha256.update(f.read())
    with open(icon_path, "rb") as f:
        sha256.update(f.read())
    with open(thumb_path, "rb") as f:
        sha256.update(f.read())
    actual_hash = sha256.hexdigest()
    if meta_data.get("content_hash") != actual_hash:
        errors.append(f"meta.json content_hash mismatch: expected {actual_hash}, got {meta_data.get('content_hash')}")

    # Check docker-compose.yml
    with open(compose_path, "r", encoding="utf-8") as f:
        compose_text = f.read()
        compose_yaml = yaml.safe_load(compose_text)

    services = compose_yaml.get("services", {})
    main_svc = services.get("michi-micro-server", {})
    env = main_svc.get("environment", [])
    env_dict = {}
    for item in env:
        if "=" in item:
            k, v = item.split("=", 1)
            env_dict[k.strip()] = v.strip()

    if env_dict.get("MICHI_DEPLOYMENT_PLATFORM") != "zimaos":
        errors.append(f"docker-compose.yml must set MICHI_DEPLOYMENT_PLATFORM=zimaos (got: {env_dict.get('MICHI_DEPLOYMENT_PLATFORM')})")

    actual_img = main_svc.get("image", "")
    if not actual_img:
        errors.append("docker-compose.yml must specify a non-empty image")
    elif expected_image and expected_image not in actual_img:
        errors.append(f"docker-compose.yml image '{actual_img}' does not match expected '{expected_image}'")
    elif not actual_img.startswith("ghcr.io/pitydah/michi-micro-server"):
        errors.append(f"docker-compose.yml image '{actual_img}' is not from official repository")

    auth_pass = env_dict.get("MICHI_AUTH_PASSWORD", "")
    if ":?" not in auth_pass:
        errors.append(f"docker-compose.yml MICHI_AUTH_PASSWORD must use fail-closed ':?' parameter expansion (got: {auth_pass})")

    if errors:
        for err in errors:
            print(f"  ❌ {err}")
        return False, errors

    print("  ✓ Local store distribution structure, metadata, content hash, and compose verified.")
    return True, []

def verify_remote_store(base_url, expected_image=None, expected_version=None):
    print(f"[2/4] Verifying remote distribution endpoints at {base_url}...")
    errors = []
    base = base_url.rstrip("/")
    store_url = f"{base}/dist/store.json"
    app_base = f"{base}/dist/apps/io.michi.micro-server"
    compose_url = f"{app_base}/docker-compose.yml"
    meta_url = f"{app_base}/meta.json"
    icon_url = f"{app_base}/assets/icon.svg"
    thumb_url = f"{app_base}/assets/thumbnail.png"
    target_version = expected_version or get_product_version()

    # 1. Fetch and validate store.json
    store_data = None
    try:
        req = urllib.request.Request(store_url, headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                errors.append(f"Remote endpoint {store_url} returned HTTP status {resp.status}")
            else:
                body = resp.read().decode("utf-8")
                try:
                    store_data = json.loads(body)
                except Exception as e:
                    errors.append(f"Remote store.json is malformed JSON: {e}")
    except Exception as e:
        errors.append(f"Failed to fetch {store_url}: {e}")

    if store_data is not None:
        if not isinstance(store_data, dict):
            errors.append("Remote store.json payload is not a JSON object")
        else:
            if store_data.get("store_id") != "io.michi.store":
                errors.append(f"Remote store.json store_id must be 'io.michi.store', got '{store_data.get('store_id')}'")
            apps = store_data.get("apps", [])
            michi_app = next((a for a in apps if isinstance(a, dict) and a.get("id") == "io.michi.micro-server"), None)
            if not michi_app:
                errors.append("Remote store.json missing app 'io.michi.micro-server'")
            else:
                app_version = michi_app.get("version")
                if app_version != target_version:
                    errors.append(f"Remote store app version '{app_version}' does not match expected '{target_version}'")
                else:
                    print(f"  ✓ Remote store.json contains io.michi.micro-server (version {app_version})")

    # 2. Fetch and validate docker-compose.yml
    compose_data = None
    compose_bytes = None
    try:
        req = urllib.request.Request(compose_url, headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                errors.append(f"Remote endpoint {compose_url} returned HTTP status {resp.status}")
            else:
                compose_bytes = resp.read()
                try:
                    compose_data = yaml.safe_load(compose_bytes.decode("utf-8"))
                except Exception as e:
                    errors.append(f"Remote docker-compose.yml is malformed YAML: {e}")
    except Exception as e:
        errors.append(f"Failed to fetch {compose_url}: {e}")

    if compose_data is not None:
        if not isinstance(compose_data, dict):
            errors.append("Remote docker-compose.yml payload is not a YAML mapping")
        else:
            services = compose_data.get("services", {})
            svc = services.get("michi-micro-server") or services.get("michi-server") or services.get("michi")
            if not svc:
                errors.append("Remote docker-compose.yml missing 'michi-micro-server' service")
            else:
                img = svc.get("image", "")
                if not img:
                    errors.append("Remote docker-compose.yml service missing 'image'")
                elif expected_image and expected_image not in img:
                    errors.append(f"Remote docker-compose.yml image '{img}' does not match expected '{expected_image}'")
                elif not img.startswith("ghcr.io/pitydah/michi-micro-server"):
                    errors.append(f"Remote docker-compose.yml image '{img}' is not from official repository")
                else:
                    print(f"  ✓ Remote docker-compose.yml image verified: {img}")

                # Check environment
                env = svc.get("environment", [])
                env_dict = {}
                if isinstance(env, dict):
                    env_dict = env
                elif isinstance(env, list):
                    for item in env:
                        if isinstance(item, str) and "=" in item:
                            k, v = item.split("=", 1)
                            env_dict[k.strip()] = v.strip()

                if env_dict.get("MICHI_DEPLOYMENT_PLATFORM") != "zimaos":
                    errors.append(f"Remote docker-compose.yml must set MICHI_DEPLOYMENT_PLATFORM=zimaos (got: {env_dict.get('MICHI_DEPLOYMENT_PLATFORM')})")
                else:
                    print("  ✓ Remote docker-compose.yml MICHI_DEPLOYMENT_PLATFORM=zimaos verified")

                auth_pass = env_dict.get("MICHI_AUTH_PASSWORD", "")
                if ":?" not in auth_pass:
                    errors.append(f"Remote docker-compose.yml MICHI_AUTH_PASSWORD must use fail-closed ':?' parameter expansion (got: {auth_pass})")
                else:
                    print("  ✓ Remote docker-compose.yml fail-closed password expression verified")

    # 3. Fetch and validate meta.json
    meta_data = None
    try:
        req = urllib.request.Request(meta_url, headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                errors.append(f"Remote endpoint {meta_url} returned HTTP status {resp.status}")
            else:
                meta_data = json.loads(resp.read().decode("utf-8"))
    except Exception as e:
        errors.append(f"Failed to fetch {meta_url}: {e}")

    if meta_data is not None:
        meta_ver = meta_data.get("version")
        if meta_ver != target_version:
            errors.append(f"Remote meta.json version '{meta_ver}' does not match expected '{target_version}'")
        else:
            print(f"  ✓ Remote meta.json version matched: {meta_ver}")

    # 4. Fetch assets and verify content_hash
    icon_bytes = None
    try:
        req = urllib.request.Request(icon_url, headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                errors.append(f"Remote endpoint {icon_url} returned HTTP status {resp.status}")
            else:
                icon_bytes = resp.read()
                if len(icon_bytes) == 0:
                    errors.append(f"Remote asset {icon_url} is empty")
    except Exception as e:
        errors.append(f"Failed to fetch {icon_url}: {e}")

    thumb_bytes = None
    try:
        req = urllib.request.Request(thumb_url, headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                errors.append(f"Remote endpoint {thumb_url} returned HTTP status {resp.status}")
            else:
                thumb_bytes = resp.read()
                if len(thumb_bytes) == 0:
                    errors.append(f"Remote asset {thumb_url} is empty")
    except Exception as e:
        errors.append(f"Failed to fetch {thumb_url}: {e}")

    if compose_bytes and icon_bytes and thumb_bytes and meta_data:
        expected_hash = meta_data.get("content_hash")
        sha256 = hashlib.sha256()
        sha256.update(compose_bytes)
        sha256.update(icon_bytes)
        sha256.update(thumb_bytes)
        actual_hash = sha256.hexdigest()
        if expected_hash != actual_hash:
            errors.append(f"Remote meta.json content_hash mismatch: expected calculated {actual_hash}, got {expected_hash}")
        else:
            print(f"  ✓ Remote content_hash verified against compose and assets: {actual_hash}")

    if errors:
        for err in errors:
            print(f"  ❌ {err}")
        return False, errors

    print("  ✓ Remote distribution endpoints verified.")
    return True, []

def login_and_get_token(server_url, username, password):
    """Authenticate with Michi Micro Server via POST /api/auth/login and return bearer token."""
    base = server_url.rstrip("/")
    login_url = f"{base}/api/auth/login"
    payload = json.dumps({"username": username, "password": password}).encode("utf-8")
    req = urllib.request.Request(
        login_url,
        data=payload,
        headers={"Content-Type": "application/json", "User-Agent": "michi-zima-verifier"},
        method="POST"
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                return None, f"Login returned HTTP status {resp.status}"
            data = json.loads(resp.read().decode("utf-8"))
            token = data.get("token")
            if not token:
                return None, "Login response missing 'token'"
            return token, None
    except urllib.error.HTTPError as e:
        return None, f"Login HTTP error {e.code}: {e.reason}"
    except Exception as e:
        return None, f"Login request failed: {e}"

def verify_running_server(
    server_url,
    expected_version=None,
    expected_commit=None,
    expected_platform=None,
    token=None,
    username=None,
    password=None,
):
    print(f"[3/4] Verifying running server at {server_url}...")
    errors = []
    base = server_url.rstrip("/")

    # Resolve token if credentials provided
    auth_token = token
    if not auth_token and username and password:
        tok, login_err = login_and_get_token(server_url, username, password)
        if login_err:
            errors.append(f"Authentication failed: {login_err}")
        else:
            auth_token = tok
            print("  ✓ Authenticated via /api/auth/login successfully")

    runtime_info = {
        "version": None,
        "commit": None,
        "deployment_platform": None,
    }

    # 1. Health check
    try:
        req = urllib.request.Request(f"{base}/health/live", headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            body = resp.read().decode()
            if resp.status != 200 or "OK" not in body:
                errors.append(f"/health/live returned status {resp.status}: {body}")
            else:
                print("  ✓ /health/live responded OK")
    except Exception as e:
        errors.append(f"Failed to reach /health/live: {e}")

    # 2. Check Cache-Control on root and sw.js
    for path in ["/", "/sw.js"]:
        try:
            req = urllib.request.Request(f"{base}{path}", headers={"User-Agent": "michi-zima-verifier"})
            with urllib.request.urlopen(req, timeout=5) as resp:
                cc = resp.headers.get("cache-control", "")
                for req_clause in ["no-cache", "no-store", "must-revalidate"]:
                    if req_clause not in cc:
                        errors.append(f"{path} missing '{req_clause}' in Cache-Control header (got: '{cc}')")
                if "no-cache" in cc and "no-store" in cc and "must-revalidate" in cc:
                    print(f"  ✓ {path} Cache-Control verified: '{cc}'")
        except Exception as e:
            errors.append(f"Failed to fetch {path}: {e}")

    # 3. Check /api/v1/server/info (public runtime identity endpoint)
    try:
        headers = {"User-Agent": "michi-zima-verifier"}
        if auth_token:
            headers["Authorization"] = f"Bearer {auth_token}"
        req = urllib.request.Request(f"{base}/api/v1/server/info", headers=headers)
        with urllib.request.urlopen(req, timeout=5) as resp:
            if resp.status != 200:
                errors.append(f"/api/v1/server/info returned status {resp.status}")
            else:
                data = json.loads(resp.read().decode())
                svc = data.get("service")
                if svc != "michi-micro-server":
                    errors.append(f"/api/v1/server/info service mismatch: expected 'michi-micro-server', got '{svc}'")
                api_ver = data.get("api_version")
                if api_ver != "v1":
                    errors.append(f"/api/v1/server/info api_version mismatch: expected 'v1', got '{api_ver}'")

                actual_ver = data.get("version")
                actual_commit = data.get("commit")
                actual_plat = data.get("deployment_platform")
                runtime_info["version"] = actual_ver
                runtime_info["commit"] = actual_commit
                runtime_info["deployment_platform"] = actual_plat

                if expected_version and actual_ver != expected_version:
                    errors.append(f"/api/v1/server/info version mismatch: expected '{expected_version}', got '{actual_ver}'")
                elif expected_version:
                    print(f"  ✓ /api/v1/server/info version matched: {actual_ver}")

                if expected_commit:
                    if not actual_commit:
                        errors.append(f"/api/v1/server/info missing commit (expected '{expected_commit}')")
                    elif actual_commit != expected_commit:
                        errors.append(f"/api/v1/server/info commit mismatch: expected '{expected_commit}', got '{actual_commit}'")
                    else:
                        print(f"  ✓ /api/v1/server/info commit matched: {actual_commit}")

                if expected_platform and actual_plat != expected_platform:
                    errors.append(f"/api/v1/server/info deployment_platform mismatch: expected '{expected_platform}', got '{actual_plat}'")
                elif expected_platform:
                    print(f"  ✓ /api/v1/server/info deployment_platform matched: {actual_plat}")
    except Exception as e:
        errors.append(f"Failed to query /api/v1/server/info: {e}")

    # 4. Check /api/v1/update/status (protected admin endpoint requiring auth when enabled)
    try:
        headers = {"User-Agent": "michi-zima-verifier"}
        if auth_token:
            headers["Authorization"] = f"Bearer {auth_token}"
        req = urllib.request.Request(f"{base}/api/v1/update/status", headers=headers)
        with urllib.request.urlopen(req, timeout=5) as resp:
            if resp.status != 200:
                errors.append(f"/api/v1/update/status returned status {resp.status}")
            else:
                data = json.loads(resp.read().decode())
                plat = data.get("deployment_platform")
                if expected_platform and plat != expected_platform:
                    errors.append(f"/api/v1/update/status deployment_platform mismatch: expected '{expected_platform}', got '{plat}'")
                elif expected_platform:
                    print(f"  ✓ /api/v1/update/status deployment_platform matched: {plat}")

                commit = data.get("commit")
                if expected_commit:
                    if not commit:
                        errors.append(f"/api/v1/update/status missing commit (expected '{expected_commit}')")
                    elif commit != expected_commit:
                        errors.append(f"/api/v1/update/status commit mismatch: expected '{expected_commit}', got '{commit}'")
                    else:
                        print(f"  ✓ /api/v1/update/status commit matched: {commit}")
    except urllib.error.HTTPError as e:
        if e.code == 401:
            errors.append("/api/v1/update/status returned 401 Unauthorized (requires valid --username/--password or --token)")
        else:
            errors.append(f"/api/v1/update/status returned HTTP status {e.code}")
    except Exception as e:
        errors.append(f"Failed to query /api/v1/update/status: {e}")

    if errors:
        for err in errors:
            print(f"  ❌ {err}")
        return False, errors, runtime_info

    print("  ✓ Running server checks passed.")
    return True, [], runtime_info

def main():
    parser = argparse.ArgumentParser(description="Verify ZimaOS package delivery and installation")
    parser.add_argument("--dist-dir", default=os.path.join(ROOT_DIR, "dist"), help="Path to dist/ directory")
    parser.add_argument("--expected-image", default=None, help="Expected Docker image or digest reference")
    parser.add_argument("--remote-url", default=None, help="Remote raw repository base URL to verify public accessibility")
    parser.add_argument("--server-url", default=None, help="URL of running Michi Micro Server instance to verify")
    parser.add_argument("--expected-version", default=None, help="Expected product version (e.g. 1.0.0-rc.2)")
    parser.add_argument("--expected-commit", default=None, help="Expected build commit SHA")
    parser.add_argument("--expected-platform", default=None, help="Expected deployment platform (e.g. zimaos)")
    parser.add_argument("--username", default=None, help="Username for authenticating against protected server endpoints")
    parser.add_argument("--password", default=None, help="Password for authenticating against protected server endpoints")
    parser.add_argument("--token", default=None, help="Pre-authenticated Bearer token for protected server endpoints")
    parser.add_argument("--output-evidence", default=None, help="Path to write JSON evidence report")
    args = parser.parse_args()

    results = {
        "local_dist_valid": False,
        "remote_endpoints_valid": None,
        "running_server_valid": None,
        "runtime": None,
        "errors": []
    }

    local_ok, local_errs = verify_local_dist(args.dist_dir, expected_image=args.expected_image)
    results["local_dist_valid"] = local_ok
    results["errors"].extend(local_errs)

    if args.remote_url:
        remote_ok, remote_errs = verify_remote_store(
            args.remote_url,
            expected_image=args.expected_image,
            expected_version=args.expected_version,
        )
        results["remote_endpoints_valid"] = remote_ok
        results["errors"].extend(remote_errs)

    if args.server_url:
        server_ok, server_errs, runtime_info = verify_running_server(
            args.server_url,
            expected_version=args.expected_version,
            expected_commit=args.expected_commit,
            expected_platform=args.expected_platform,
            token=args.token,
            username=args.username,
            password=args.password,
        )
        results["running_server_valid"] = server_ok
        results["runtime"] = runtime_info
        results["errors"].extend(server_errs)

    passed = len(results["errors"]) == 0
    results["status"] = "PASS" if passed else "FAIL"

    if args.output_evidence:
        os.makedirs(os.path.dirname(os.path.abspath(args.output_evidence)), exist_ok=True)
        with open(args.output_evidence, "w", encoding="utf-8") as f:
            json.dump(results, f, indent=2)
        print(f"Evidence written to {args.output_evidence}")

    if not passed:
        print(f"\nVerification FAILED with {len(results['errors'])} error(s).")
        sys.exit(1)

    print("\n✓ ALL ZIMAOS VERIFICATIONS PASSED SUCCESSFULLY.")
    sys.exit(0)

if __name__ == "__main__":
    main()
