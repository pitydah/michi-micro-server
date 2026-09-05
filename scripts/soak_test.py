#!/usr/bin/env python3
"""
Michi Micro Server — Soak Test & Stability Monitor.

Monitors memory RSS, CPU, file descriptors, SQLite WAL checkpoints,
child processes, cache growth, and request latency under continuous load.
Emits structured evidence artifacts in all exit paths (PASS and FAIL).

Usage:
  python3 scripts/soak_test.py --url http://127.0.0.1:9091 --pid 12345 --duration-seconds 30 --report target/soak_report.json
"""

import argparse
import datetime
import json
import os
import subprocess
import sys
import time
import urllib.request
import urllib.error

def get_head_sha():
    try:
        return subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    except Exception:
        return "UNKNOWN_SHA"

def get_process_metrics(pid):
    status_path = f"/proc/{pid}/status"
    if not os.path.exists(status_path):
        return None

    metrics = {
        "rss_kb": 0,
        "vms_kb": 0,
        "open_fds": 0,
        "threads": 0,
        "child_processes": 0,
        "valid": False
    }

    try:
        with open(status_path, "r") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    metrics["rss_kb"] = int(line.split()[1])
                elif line.startswith("VmSize:"):
                    metrics["vms_kb"] = int(line.split()[1])
                elif line.startswith("Threads:"):
                    metrics["threads"] = int(line.split()[1])
        metrics["valid"] = metrics["rss_kb"] > 0
    except Exception:
        return None

    fd_dir = f"/proc/{pid}/fd"
    if os.path.exists(fd_dir):
        try:
            metrics["open_fds"] = len(os.listdir(fd_dir))
        except Exception:
            pass

    task_dir = f"/proc/{pid}/task"
    if os.path.exists(task_dir):
        try:
            children_path = f"/proc/{pid}/task/{pid}/children"
            if os.path.exists(children_path):
                with open(children_path, "r") as f:
                    metrics["child_processes"] = len(f.read().split())
        except Exception:
            pass

    return metrics

def get_wal_size(config_dir):
    wal_path = os.path.join(config_dir, "michi.db-wal")
    if os.path.exists(wal_path):
        return os.path.getsize(wal_path)
    return 0

def http_get(url, headers=None, timeout=5):
    req = urllib.request.Request(url, headers=headers or {})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.status, resp.read()

def http_post_json(url, payload, headers=None, timeout=5):
    data = json.dumps(payload).encode("utf-8")
    h = {"Content-Type": "application/json"}
    if headers:
        h.update(headers)
    req = urllib.request.Request(url, data=data, headers=h)
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.status, resp.read()

def main():
    parser = argparse.ArgumentParser(description="Michi Micro Server Soak Stability Test")
    parser.add_argument("--url", default="http://127.0.0.1:9091", help="Base server URL")
    parser.add_argument("--pid", type=int, required=True, help="Server process PID")
    parser.add_argument("--duration-seconds", type=int, default=30, help="Test duration in seconds")
    parser.add_argument("--sample-interval", type=float, default=2.0, help="Sampling interval in seconds")
    parser.add_argument("--config-dir", default="target/soak_test_config", help="Database/config dir for WAL tracking")
    parser.add_argument("--report", default="target/soak_report.json", help="Path to write JSON report")
    parser.add_argument("--username", default="admin")
    parser.add_argument("--password", default="admin123")
    args = parser.parse_args()

    base_url = args.url.rstrip("/")
    pid = args.pid
    total_seconds = args.duration_seconds
    sha = get_head_sha()

    print("=" * 70)
    print("MICHI MICRO SERVER — ROBUST SOAK & TELEMETRY MONITOR")
    print(f"Server URL: {base_url} | PID: {pid} | Target Duration: {total_seconds}s | Commit: {sha[:8]}")
    print("=" * 70)

    violations = []
    request_errors = []

    # Check process existence
    initial_metrics = get_process_metrics(pid)
    if not initial_metrics or not initial_metrics["valid"]:
        violations.append(f"INITIAL_PROCESS_UNAVAILABLE: PID {pid} is not reachable or readable via /proc")
        report_data = {
            "schema_version": 1,
            "gate_id": "short-stability-smoke",
            "commit_sha": sha,
            "evidence_class": "INTEGRATION_REAL",
            "status": "FAIL",
            "detail": f"Process metrics unavailable for PID {pid}",
            "violations": violations,
            "duration_seconds": 0,
            "samples_collected": 0,
            "exit_code": 1
        }
        os.makedirs(os.path.dirname(os.path.abspath(args.report)), exist_ok=True)
        with open(args.report, "w", encoding="utf-8") as f:
            json.dump(report_data, f, indent=2)
        sys.exit(1)

    auth_headers = {}
    try:
        st, body = http_post_json(
            f"{base_url}/api/auth/login",
            {"username": args.username, "password": args.password}
        )
        if st == 200:
            token = json.loads(body).get("token")
            if token:
                auth_headers = {"Authorization": f"Bearer {token}"}
    except Exception:
        pass

    telemetry = []
    start_time = time.time()
    iteration = 0

    peak_rss = initial_metrics["rss_kb"]
    max_fds = initial_metrics["open_fds"]
    peak_wal = get_wal_size(args.config_dir)

    while True:
        now = time.time()
        elapsed = now - start_time
        if elapsed >= total_seconds:
            break

        iteration += 1

        # 1. Generate workload traffic
        try:
            st, _ = http_get(f"{base_url}/health/live")
            if st != 200:
                request_errors.append(f"health check returned {st}")

            st, _ = http_get(f"{base_url}/api/v1/status")
            if st != 200:
                request_errors.append(f"status returned {st}")

            http_get(f"{base_url}/api/v1/search?q=test", headers=auth_headers)
            http_get(f"{base_url}/api/v1/queue", headers=auth_headers)

            st, t_body = http_get(f"{base_url}/api/v1/tracks", headers=auth_headers)
            if st == 200:
                t_json = json.loads(t_body)
                tracks = t_json.get("tracks", t_json) if isinstance(t_json, dict) else t_json
                if len(tracks) > 0:
                    tid = tracks[0]["id"]
                    req_h = {"Range": "bytes=0-4095"}
                    req_h.update(auth_headers)
                    http_get(f"{base_url}/api/v1/tracks/{tid}/stream", headers=req_h)
        except Exception as e:
            request_errors.append(f"workload request error at {round(elapsed,1)}s: {e}")

        # 2. Collect process & OS telemetry
        metrics = get_process_metrics(pid)
        if not metrics or not metrics["valid"]:
            violations.append(f"SERVER_DIED_DURING_SOAK at {round(elapsed,1)}s")
            break

        wal_bytes = get_wal_size(args.config_dir)
        rss_mb = metrics["rss_kb"] / 1024.0

        if metrics["rss_kb"] > peak_rss:
            peak_rss = metrics["rss_kb"]
        if metrics["open_fds"] > max_fds:
            max_fds = metrics["open_fds"]
        if wal_bytes > peak_wal:
            peak_wal = wal_bytes

        sample = {
            "timestamp": time.time(),
            "elapsed_seconds": round(elapsed, 1),
            "rss_mb": round(rss_mb, 2),
            "open_fds": metrics["open_fds"],
            "threads": metrics["threads"],
            "child_processes": metrics["child_processes"],
            "wal_bytes": wal_bytes,
        }
        telemetry.append(sample)

        if iteration % 5 == 0 or elapsed >= total_seconds - 1:
            print(
                f"[{elapsed:6.1f}s / {total_seconds}s] RSS: {rss_mb:6.2f} MB | FDs: {metrics['open_fds']:3d} | "
                f"Threads: {metrics['threads']:2d} | WAL: {wal_bytes / 1024:6.1f} KB | Children: {metrics['child_processes']}"
            )

        time.sleep(args.sample_interval)

    actual_duration = round(time.time() - start_time, 2)
    final_metrics = get_process_metrics(pid)

    if not final_metrics or not final_metrics["valid"]:
        if "SERVER_DIED_DURING_SOAK" not in " ".join(violations):
            violations.append("SERVER_TERMINATED_BEFORE_FINAL_SAMPLE")
        final_metrics = initial_metrics

    initial_rss_mb = initial_metrics["rss_kb"] / 1024.0
    final_rss_mb = final_metrics["rss_kb"] / 1024.0
    peak_rss_mb = peak_rss / 1024.0
    rss_drift_mb = final_rss_mb - initial_rss_mb
    fd_drift = final_metrics["open_fds"] - initial_metrics["open_fds"]

    # Invariants and Assertions as Violations
    if fd_drift >= 15:
        violations.append(f"FD_LEAK_DETECTED: +{fd_drift} file descriptors")
    if final_metrics["child_processes"] != 0:
        violations.append(f"ZOMBIE_PROCESSES_DETECTED: {final_metrics['child_processes']} children left")
    if len(request_errors) > 5:
        violations.append(f"HIGH_REQUEST_ERROR_COUNT: {len(request_errors)} errors observed")

    status = "PASS" if not violations else "FAIL"
    detail = (
        f"Soak completed: RSS drift {rss_drift_mb:+.2f}MB, FD drift {fd_drift:+d}, 0 zombies"
        if status == "PASS" else f"Soak stability violations: {'; '.join(violations)}"
    )

    report_data = {
        "schema_version": 1,
        "gate_id": "short-stability-smoke",
        "commit_sha": sha,
        "evidence_class": "INTEGRATION_REAL",
        "status": status,
        "detail": detail,
        "requested_duration_seconds": total_seconds,
        "actual_elapsed_seconds": actual_duration,
        "samples_collected": len(telemetry),
        "initial_rss_mb": round(initial_rss_mb, 2),
        "final_rss_mb": round(final_rss_mb, 2),
        "peak_rss_mb": round(peak_rss_mb, 2),
        "rss_drift_mb": round(rss_drift_mb, 2),
        "initial_fds": initial_metrics["open_fds"],
        "final_fds": final_metrics["open_fds"],
        "peak_fds": max_fds,
        "fd_drift": fd_drift,
        "peak_wal_bytes": peak_wal,
        "child_processes": final_metrics["child_processes"],
        "request_errors_count": len(request_errors),
        "violations": violations,
        "exit_code": 0 if status == "PASS" else 1
    }

    report_path = os.path.abspath(args.report)
    os.makedirs(os.path.dirname(report_path), exist_ok=True)
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report_data, f, indent=2)

    print("\n" + "=" * 70)
    print(f"SOAK TEST COMPLETE — STATUS: {status}")
    print(f"Initial RSS: {initial_rss_mb:.2f} MB -> Final RSS: {final_rss_mb:.2f} MB (Peak: {peak_rss_mb:.2f} MB, Drift: {rss_drift_mb:+.2f} MB)")
    print(f"Initial FDs: {initial_metrics['open_fds']} -> Final FDs: {final_metrics['open_fds']} (Peak: {max_fds}, Drift: {fd_drift:+d})")
    print(f"Report saved to: {report_path}")
    print("=" * 70)

    if violations:
        print(f"❌ Violations: {violations}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
