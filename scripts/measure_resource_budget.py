#!/usr/bin/env python3
"""
Michi Micro Server — Resource Budget Measurement & Qualification Suite.

Measures startup latency, idle memory RSS (median & p95 over 30 samples),
active threads, and open file descriptors under realistic warm-up.
Emits structured evidence artifact and updates docs/RESOURCE_BUDGET.md.

Usage:
  python3 scripts/measure_resource_budget.py [--port 9099] [--report target/release-evidence/resource-budget.json] [--update-doc]
"""

import argparse
import datetime
import json
import os
import signal
import statistics
import subprocess
import sys
import time
import urllib.request

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def get_head_sha():
    try:
        return subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    except Exception:
        return "UNKNOWN_SHA"

def read_process_status(pid):
    status_path = f"/proc/{pid}/status"
    if not os.path.exists(status_path):
        return None
    metrics = {"rss_kb": 0, "threads": 0}
    try:
        with open(status_path, "r") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    metrics["rss_kb"] = int(line.split()[1])
                elif line.startswith("Threads:"):
                    metrics["threads"] = int(line.split()[1])
    except Exception:
        return None
    return metrics

def read_process_fds(pid):
    fd_dir = f"/proc/{pid}/fd"
    if not os.path.exists(fd_dir):
        return 0
    try:
        return len(os.listdir(fd_dir))
    except Exception:
        return 0

def wait_for_server(port, timeout=15.0):
    start = time.time()
    while time.time() - start < timeout:
        try:
            req = urllib.request.Request(f"http://127.0.0.1:{port}/health/live")
            with urllib.request.urlopen(req, timeout=1.0) as resp:
                if resp.status == 200:
                    return round((time.time() - start) * 1000, 1)
        except Exception:
            pass
        time.sleep(0.05)
    return None

def main():
    parser = argparse.ArgumentParser(description="Resource Budget Qualification")
    parser.add_argument("--port", type=int, default=9099)
    parser.add_argument("--report", default=os.path.join(ROOT_DIR, "target", "release-evidence", "resource-budget.json"))
    parser.add_argument("--update-doc", action="store_true", default=True)
    args = parser.parse_args()

    sha = get_head_sha()
    print("=" * 70)
    print("MICHI MICRO SERVER — RESOURCE BUDGET QUALIFICATION")
    print(f"Port: {args.port} | Commit: {sha[:8]}")
    print("=" * 70)

    # Ensure binary is built
    bin_path = os.path.join(ROOT_DIR, "target", "release", "michi-server")
    if not os.path.exists(bin_path):
        print("Building target/release/michi-server...")
        subprocess.check_call(["cargo", "build", "--release", "--bin", "michi-server"])

    tmp_dir = os.path.join(ROOT_DIR, "target", f"resource_test_{int(time.time())}")
    os.makedirs(tmp_dir, exist_ok=True)
    cfg_dir = os.path.join(tmp_dir, "config")
    cache_dir = os.path.join(tmp_dir, "cache")
    music_dir = os.path.join(tmp_dir, "music")
    os.makedirs(cfg_dir, exist_ok=True)
    os.makedirs(cache_dir, exist_ok=True)
    os.makedirs(music_dir, exist_ok=True)

    env = os.environ.copy()
    env["MICHI_PORT"] = str(args.port)
    env["MICHI_CONFIG_PATH"] = cfg_dir
    env["MICHI_CACHE_PATH"] = cache_dir
    env["MICHI_MUSIC_PATH"] = music_dir
    env["MICHI_DATABASE"] = f"sqlite://{cfg_dir}/michi.db"
    env["MICHI_RESOURCE_PROFILE"] = "balanced"
    env["RUST_LOG"] = "error"

    start_launch = time.time()
    proc = subprocess.Popen([bin_path], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    try:
        startup_ms = wait_for_server(args.port, timeout=15.0)
        if startup_ms is None:
            print("ERROR: Server failed to reach /health/live within timeout", file=sys.stderr)
            sys.exit(1)

        print(f"✓ Server ready at http://127.0.0.1:{args.port}/health/live (Startup Latency: {startup_ms:.1f}ms)")
        print("Warm-up settling (5s)...")
        time.sleep(5.0)

        rss_samples = []
        threads_samples = []
        fds_samples = []

        print("Sampling 30 idle metrics at 1s intervals...")
        for i in range(30):
            st = read_process_status(proc.pid)
            fds = read_process_fds(proc.pid)
            if st and st["rss_kb"] > 0:
                rss_mb = round(st["rss_kb"] / 1024.0, 2)
                rss_samples.append(rss_mb)
                threads_samples.append(st["threads"])
                fds_samples.append(fds)
            time.sleep(1.0)

        if not rss_samples:
            print("ERROR: Could not read /proc metrics for server process", file=sys.stderr)
            sys.exit(1)

        rss_median = round(statistics.median(rss_samples), 2)
        rss_p95 = round(statistics.quantiles(rss_samples, n=20)[18], 2) if len(rss_samples) >= 20 else max(rss_samples)
        rss_max = max(rss_samples)
        threads_p95 = max(threads_samples)
        fds_p95 = max(fds_samples)

        print(f"Metrics (Balanced profile):")
        print(f"  - Idle RSS Median: {rss_median} MB")
        print(f"  - Idle RSS p95:    {rss_p95} MB")
        print(f"  - Idle RSS Max:    {rss_max} MB")
        print(f"  - Threads (p95):   {threads_p95}")
        print(f"  - FDs (p95):       {fds_p95}")

        # Invariants Check
        violations = []
        if rss_p95 > 50.0:
            violations.append(f"IDLE_RSS_EXCEEDS_BUDGET: {rss_p95}MB > 50.0MB")
        if threads_p95 > 16:
            violations.append(f"THREADS_EXCEED_BUDGET: {threads_p95} > 16")

        status = "PASS" if not violations else "FAIL"
        detail = (
            f"Qualified: Idle RSS median {rss_median}MB, p95 {rss_p95}MB (<50MB target PASS), threads {threads_p95}"
            if status == "PASS" else f"Violations: {'; '.join(violations)}"
        )

        artifact = {
            "schema_version": 1,
            "gate_id": "resource-budget",
            "commit_sha": sha,
            "evidence_class": "INTEGRATION_REAL",
            "status": status,
            "detail": detail,
            "metrics": {
                "startup_ms": startup_ms,
                "idle_rss_median_mb": rss_median,
                "idle_rss_p95_mb": rss_p95,
                "idle_rss_max_mb": rss_max,
                "threads_p95": threads_p95,
                "fds_p95": fds_p95
            },
            "thresholds": {
                "idle_rss_mb_max": 50.0,
                "threads_max": 16
            },
            "violations": violations,
            "exit_code": 0 if status == "PASS" else 1
        }

        os.makedirs(os.path.dirname(os.path.abspath(args.report)), exist_ok=True)
        with open(args.report, "w", encoding="utf-8") as f:
            json.dump(artifact, f, indent=2)
        print(f"Artifact written to {args.report}")

        if args.update_doc:
            doc_path = os.path.join(ROOT_DIR, "docs", "RESOURCE_BUDGET.md")
            if os.path.exists(doc_path):
                now_str = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
                env_str = f"Linux {os.uname().machine} ({now_str}, commit {sha[:8]})"
                with open(doc_path, "r", encoding="utf-8") as f:
                    doc = f.read()

                old_row = "| Memory (idle) | < 50 MB | -- | -- |"
                new_row = f"| Memory (idle) | < 50 MB | **{rss_p95:.1f} MB** (median {rss_median:.1f} MB) | {env_str} |"
                if old_row in doc:
                    doc = doc.replace(old_row, new_row)
                    with open(doc_path, "w", encoding="utf-8") as f:
                        f.write(doc)
                    print("Updated docs/RESOURCE_BUDGET.md with certified measurements.")

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()

if __name__ == "__main__":
    main()
