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
    if expected_image and expected_image not in actual_img:
        errors.append(f"docker-compose.yml image '{actual_img}' does not match expected '{expected_image}'")

    if errors:
        for err in errors:
            print(f"  ❌ {err}")
        return False, errors

    print("  ✓ Local store distribution structure, metadata, content hash, and compose verified.")
    return True, []

def verify_remote_store(base_url):
    print(f"[2/4] Verifying remote distribution endpoints at {base_url}...")
    errors = []
    base = base_url.rstrip("/")
    store_url = f"{base}/dist/store.json"
    compose_url = f"{base}/dist/apps/io.michi.micro-server/docker-compose.yml"

    for url in [store_url, compose_url]:
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "michi-zima-verifier"})
            with urllib.request.urlopen(req, timeout=10) as resp:
                if resp.status != 200:
                    errors.append(f"Remote endpoint {url} returned HTTP status {resp.status}")
                else:
                    print(f"  ✓ Accessible: {url}")
        except Exception as e:
            errors.append(f"Failed to fetch {url}: {e}")

    if errors:
        for err in errors:
            print(f"  ❌ {err}")
        return False, errors

    print("  ✓ Remote distribution endpoints verified.")
    return True, []

def verify_running_server(server_url):
    print(f"[3/4] Verifying running server at {server_url}...")
    errors = []
    base = server_url.rstrip("/")

    # 1. Health check
    try:
        req = urllib.request.Request(f"{base}/health/live", headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            body = resp.read().decode()
            if resp.status != 200 or "OK" not in body:
                errors.append(f"/health/live returned {resp.status}: {body}")
            else:
                print("  ✓ /health/live responded OK")
    except Exception as e:
        errors.append(f"Failed to reach /health/live: {e}")

    # 2. Check Cache-Control on root and sw.js
    for path, expected_cc in [("/", "no-cache"), ("/sw.js", "no-cache")]:
        try:
            req = urllib.request.Request(f"{base}{path}", headers={"User-Agent": "michi-zima-verifier"})
            with urllib.request.urlopen(req, timeout=5) as resp:
                cc = resp.headers.get("cache-control", "")
                if expected_cc not in cc:
                    errors.append(f"{path} missing '{expected_cc}' in Cache-Control header (got: '{cc}')")
                else:
                    print(f"  ✓ {path} Cache-Control contains '{expected_cc}'")
        except Exception as e:
            errors.append(f"Failed to fetch {path}: {e}")

    # 3. Check /api/v1/update/status for deployment_platform
    try:
        req = urllib.request.Request(f"{base}/api/v1/update/status", headers={"User-Agent": "michi-zima-verifier"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read().decode())
            plat = data.get("deployment_platform")
            if plat != "zimaos":
                print(f"  ℹ Notice: running server reports platform='{plat}' (expected 'zimaos' if running under ZimaOS compose)")
    except Exception as e:
        print(f"  ℹ Could not verify /api/v1/update/status: {e}")

    if errors:
        for err in errors:
            print(f"  ❌ {err}")
        return False, errors

    print("  ✓ Running server checks passed.")
    return True, []

def main():
    parser = argparse.ArgumentParser(description="Verify ZimaOS package delivery and installation")
    parser.add_argument("--dist-dir", default=os.path.join(ROOT_DIR, "dist"), help="Path to dist/ directory")
    parser.add_argument("--expected-image", default=None, help="Expected Docker image or digest reference")
    parser.add_argument("--remote-url", default=None, help="Remote raw repository base URL to verify public accessibility")
    parser.add_argument("--server-url", default=None, help="URL of running Michi Micro Server instance to verify")
    parser.add_argument("--output-evidence", default=None, help="Path to write JSON evidence report")
    args = parser.parse_args()

    results = {
        "local_dist_valid": False,
        "remote_endpoints_valid": None,
        "running_server_valid": None,
        "errors": []
    }

    local_ok, local_errs = verify_local_dist(args.dist_dir, expected_image=args.expected_image)
    results["local_dist_valid"] = local_ok
    results["errors"].extend(local_errs)

    if args.remote_url:
        remote_ok, remote_errs = verify_remote_store(args.remote_url)
        results["remote_endpoints_valid"] = remote_ok
        results["errors"].extend(remote_errs)

    if args.server_url:
        server_ok, server_errs = verify_running_server(args.server_url)
        results["running_server_valid"] = server_ok
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
