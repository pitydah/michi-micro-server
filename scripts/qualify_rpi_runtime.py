#!/usr/bin/env python3
"""
Physical Hardware Runtime Qualification for Raspberry Pi 4 / 5.

Performs strict hardware and runtime qualification:
1. Inspects /proc/device-tree/model to prove physical Raspberry Pi 4 or 5 hardware.
2. Starts native michi-server binary in isolated temporary environment.
3. Verifies readiness (/health/live == 200, /api/v1/server/info == valid).
4. Verifies database migrations and schema invariants (version == 49).
5. Executes streaming smoke (scans mock audio, streams audio bytes over HTTP Range 206).
6. Captures mini resource sanity (RSS ceiling, thread count, process survival).
7. Emits rich physical hardware evidence artifact.
"""

import argparse
import json
import os
import platform
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def create_mock_flac_file(path: str, title: str, artist: str, album: str) -> int:
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    header = b"fLaC\x00\x00\x00\"\x10\x00\x10\x00\x00\x00\x00\x00\x00\x00\x0a\xc4\x42\xf0\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
    pcm_payload = (title.encode("utf-8") + b" - " + artist.encode("utf-8") + b" rpi audio stream ") * 1024
    with open(path, "wb") as f:
        f.write(header + pcm_payload)
    return len(header + pcm_payload)

def get_rpi_model(model_path: str = "/proc/device-tree/model") -> str:
    if not os.path.exists(model_path):
        raise RuntimeError(f"Device tree model file not found at {model_path}. Not running on physical ARM board.")
    try:
        with open(model_path, "rb") as f:
            raw = f.read()
        model = raw.decode("utf-8", errors="replace").replace("\x00", "").strip()
        return model
    except Exception as e:
        raise RuntimeError(f"Failed to read device tree model: {e}")

def get_process_metrics(pid: int):
    rss_bytes = 0
    threads = 0
    status_path = f"/proc/{pid}/status"
    if os.path.exists(status_path):
        try:
            with open(status_path, "r", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("VmRSS:"):
                        parts = line.split()
                        if len(parts) >= 2:
                            rss_bytes = int(parts[1]) * 1024
                    elif line.startswith("Threads:"):
                        parts = line.split()
                        if len(parts) >= 2:
                            threads = int(parts[1])
        except Exception:
            pass
    return rss_bytes, threads

def main():
    parser = argparse.ArgumentParser(description="Qualify Raspberry Pi Physical Hardware and Runtime")
    parser.add_argument("--binary", default=os.path.join(ROOT_DIR, "target", "release", "michi-server"), help="Path to michi-server binary")
    parser.add_argument("--model-path", default="/proc/device-tree/model", help="Path to device tree model file")
    parser.add_argument("--port", type=int, default=9095, help="Port to bind qualification server")
    parser.add_argument("--output-evidence", required=True, help="Path to write rich physical evidence JSON")
    parser.add_argument("--skip-model-check", action="store_true", help="Skip physical model verification (for test harness only)")
    args = parser.parse_args()

    evidence = {
        "status": "FAIL",
        "hardware_model": None,
        "architecture": platform.machine(),
        "kernel": platform.release(),
        "runtime_version": None,
        "runtime_commit": None,
        "rss_bytes": None,
        "thread_count": None,
        "database_schema_version": None,
        "health_result": "NOT_RUN",
        "stream_smoke_result": "NOT_RUN",
        "errors": [],
    }

    # 1. P0-05: Hardware Model Verification
    if not args.skip_model_check:
        print(f"[1/5] Inspecting physical board identity at {args.model_path}...")
        try:
            model = get_rpi_model(args.model_path)
            evidence["hardware_model"] = model
            print(f"  ✓ Detected hardware: {model}")
            if "Raspberry Pi 4" not in model and "Raspberry Pi 5" not in model:
                err = f"Unaccepted physical hardware model: '{model}'. Qualification requires Raspberry Pi 4 or 5."
                print(f"  ❌ {err}", file=sys.stderr)
                evidence["errors"].append(err)
                with open(args.output_evidence, "w", encoding="utf-8") as f:
                    json.dump(evidence, f, indent=2)
                sys.exit(1)
        except Exception as e:
            err = f"Hardware platform check failed: {e}"
            print(f"  ❌ {err}", file=sys.stderr)
            evidence["errors"].append(err)
            with open(args.output_evidence, "w", encoding="utf-8") as f:
                json.dump(evidence, f, indent=2)
            sys.exit(1)
    else:
        evidence["hardware_model"] = "Raspberry Pi 5 Model B (Test Override)"

    # 2. Prepare Isolated Runtime Directories
    temp_dir = tempfile.mkdtemp(prefix="michi_rpi_qual_")
    config_dir = os.path.join(temp_dir, "config")
    cache_dir = os.path.join(temp_dir, "cache")
    music_dir = os.path.join(temp_dir, "music")
    for d in [config_dir, cache_dir, music_dir]:
        os.makedirs(d, exist_ok=True)

    # Seed mock audio
    track_path = os.path.join(music_dir, "rpi_stream_smoke.flac")
    create_mock_flac_file(track_path, "RPi Physical Stream", "Michi ARM64", "Physical Qualification")

    # 3. Start Native michi-server
    print(f"[2/5] Starting native michi-server at port {args.port}...")
    env = os.environ.copy()
    env["MICHI_CONFIG_DIR"] = config_dir
    env["MICHI_CACHE_DIR"] = cache_dir
    env["MICHI_MUSIC_DIR"] = music_dir
    env["MICHI_SERVER_PORT"] = str(args.port)
    env["MICHI_DEPLOYMENT_PLATFORM"] = "rpi"
    env["RUST_LOG"] = "info"

    proc = None
    try:
        proc = subprocess.Popen(
            [args.binary],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        # 4. Wait for readiness
        base_url = f"http://127.0.0.1:{args.port}"
        ready = False
        start_time = time.time()
        while time.time() - start_time < 20:
            if proc.poll() is not None:
                _, stderr = proc.communicate()
                evidence["errors"].append(f"michi-server process terminated prematurely: {stderr.decode()}")
                break
            try:
                req = urllib.request.Request(f"{base_url}/health/live")
                with urllib.request.urlopen(req, timeout=1) as resp:
                    if resp.status == 200:
                        ready = True
                        break
            except Exception:
                time.sleep(0.5)

        if not ready:
            err = "michi-server failed to respond to /health/live within 20 seconds"
            evidence["errors"].append(err)
            raise RuntimeError(err)

        evidence["health_result"] = "PASS"
        print(f"  ✓ /health/live responded OK ({int((time.time() - start_time) * 1000)}ms)")

        # 5. Runtime server info check
        req = urllib.request.Request(f"{base_url}/api/v1/server/info")
        with urllib.request.urlopen(req, timeout=5) as resp:
            info = json.loads(resp.read().decode())
            evidence["runtime_version"] = info.get("version")
            evidence["runtime_commit"] = info.get("commit")
            print(f"  ✓ /api/v1/server/info verified: version={info.get('version')}, commit={info.get('commit')}")

        # 6. Database schema version verification (P0-06)
        print("[3/5] Verifying SQLite database migrations and schema...")
        db_path = os.path.join(config_dir, "michi.db")
        if not os.path.exists(db_path):
            raise RuntimeError(f"michi.db not found at {db_path}")
        conn = sqlite3.connect(db_path)
        cur = conn.cursor()
        cur.execute("SELECT MAX(version) FROM _migrations")
        max_ver = cur.fetchone()[0]
        conn.close()
        evidence["database_schema_version"] = max_ver
        if max_ver != 49:
            err = f"Expected database migration schema 49, got {max_ver}"
            evidence["errors"].append(err)
            raise RuntimeError(err)
        print(f"  ✓ Database migrated successfully to schema {max_ver}")

        # 7. Streaming Smoke Verification (P0-07)
        print("[4/5] Executing streaming smoke test...")
        scan_req = urllib.request.Request(
            f"{base_url}/api/v1/library/scan",
            data=json.dumps({}).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(scan_req, timeout=10) as resp:
            assert resp.status == 200, f"Library scan failed: {resp.status}"

        # Poll tracks until scanned track is present
        tracks = []
        for _ in range(10):
            with urllib.request.urlopen(f"{base_url}/api/v1/tracks", timeout=5) as resp:
                data = json.loads(resp.read().decode())
                tracks = data.get("tracks", data) if isinstance(data, dict) else data
                if len(tracks) > 0:
                    break
            time.sleep(0.5)

        if len(tracks) == 0:
            err = "Library scan did not index test audio fixture"
            evidence["errors"].append(err)
            raise RuntimeError(err)

        track_id = tracks[0]["id"]
        stream_req = urllib.request.Request(
            f"{base_url}/api/v1/tracks/{track_id}/stream",
            headers={"Range": "bytes=0-1023"},
        )
        with urllib.request.urlopen(stream_req, timeout=10) as resp:
            audio_bytes = resp.read()
            if resp.status not in (200, 206) or len(audio_bytes) == 0:
                err = f"Streaming returned HTTP {resp.status} with {len(audio_bytes)} bytes"
                evidence["errors"].append(err)
                raise RuntimeError(err)
            print(f"  ✓ Stream smoke succeeded: HTTP {resp.status}, {len(audio_bytes)} bytes received")
            evidence["stream_smoke_result"] = "PASS"

        # 8. Resource Sanity (P0-06, P0-08)
        print("[5/5] Measuring runtime resource sanity...")
        rss, threads = get_process_metrics(proc.pid)
        evidence["rss_bytes"] = rss
        evidence["thread_count"] = threads
        print(f"  ✓ Process alive: PID={proc.pid}, RSS={rss // 1024} KB, Threads={threads}")

        # RSS hard limit: 512 MB, thread ceiling: 64
        if rss > 512 * 1024 * 1024:
            err = f"RSS usage {rss} bytes exceeded 512MB hard ceiling"
            evidence["errors"].append(err)
            raise RuntimeError(err)
        if threads > 64:
            err = f"Thread count {threads} exceeded ceiling of 64"
            evidence["errors"].append(err)
            raise RuntimeError(err)

        if len(evidence["errors"]) == 0:
            evidence["status"] = "PASS"

    except Exception as e:
        print(f"  ❌ Qualification failed: {e}", file=sys.stderr)
        if str(e) not in evidence["errors"]:
            evidence["errors"].append(str(e))
    finally:
        if proc and proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    os.makedirs(os.path.dirname(os.path.abspath(args.output_evidence)), exist_ok=True)
    with open(args.output_evidence, "w", encoding="utf-8") as f:
        json.dump(evidence, f, indent=2)

    print(f"\nWritten Raspberry Pi physical qualification evidence to {args.output_evidence}")
    if evidence["status"] != "PASS":
        sys.exit(1)
    print("✓ RASPBERRY PI PHYSICAL QUALIFICATION PASSED.")
    sys.exit(0)

if __name__ == "__main__":
    main()
