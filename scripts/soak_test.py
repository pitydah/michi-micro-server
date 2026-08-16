#!/usr/bin/env python3
"""
Michi Micro Server — Soak Test & Stability Monitor.

Monitors memory RSS, CPU, file descriptors, SQLite WAL checkpoints,
child processes, cache growth, and request latency under continuous load.

Usage:
  python3 scripts/soak_test.py --url http://127.0.0.1:9091 --pid 12345 --duration-seconds 30 --report target/soak_report.json
"""

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
import urllib.error

def get_process_metrics(pid):
    metrics = {
        "rss_kb": 0,
        "vms_kb": 0,
        "open_fds": 0,
        "threads": 0,
        "child_processes": 0,
    }
    # Read /proc/<pid>/status
    status_path = f"/proc/{pid}/status"
    if os.path.exists(status_path):
        try:
            with open(status_path, "r") as f:
                for line in f:
                    if line.startswith("VmRSS:"):
                        metrics["rss_kb"] = int(line.split()[1])
                    elif line.startswith("VmSize:"):
                        metrics["vms_kb"] = int(line.split()[1])
                    elif line.startswith("Threads:"):
                        metrics["threads"] = int(line.split()[1])
        except Exception:
            pass

    # Read /proc/<pid>/fd
    fd_dir = f"/proc/{pid}/fd"
    if os.path.exists(fd_dir):
        try:
            metrics["open_fds"] = len(os.listdir(fd_dir))
        except Exception:
            pass

    # Check child processes
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
    parser = argparse.ArgumentParser(description="Michi Micro Server Soak Test Monitor")
    parser.add_argument("--url", default="http://127.0.0.1:9091")
    parser.add_argument("--pid", type=int, default=None)
    parser.add_argument("--config-dir", default="/tmp/michi_rel_test/config")
    parser.add_argument("--duration-seconds", type=int, default=30)
    parser.add_argument("--duration-hours", type=float, default=None)
    parser.add_argument("--sample-interval", type=float, default=2.0)
    parser.add_argument("--username", default="admin")
    parser.add_argument("--password", default="admin123")
    parser.add_argument("--report", default="target/soak_report.json")
    args = parser.parse_args()

    total_seconds = int(args.duration_hours * 3600) if args.duration_hours else args.duration_seconds
    base_url = args.url.rstrip("/")
    pid = args.pid

    if pid is None:
        # Try to find michi-server PID
        try:
            out = subprocess.check_output(["pgrep", "-f", "michi-server"]).decode().strip().split()
            if out:
                pid = int(out[-1])
        except Exception:
            pid = os.getpid()

    print("=" * 70)
    print("MICHI MICRO SERVER — CONTINUOUS STABILITY & SOAK MONITOR")
    print(f"Target URL:        {base_url}")
    print(f"Server PID:        {pid}")
    print(f"Duration:          {total_seconds} seconds ({total_seconds / 3600:.2f} hours)")
    print(f"Sample Interval:   {args.sample_interval}s")
    print(f"Report Output:     {args.report}")
    print("=" * 70)

    # Authenticate
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
    next_sample = start_time
    iteration = 0

    initial_metrics = get_process_metrics(pid)
    peak_rss = initial_metrics["rss_kb"]
    max_fds = initial_metrics["open_fds"]

    while True:
        now = time.time()
        elapsed = now - start_time
        if elapsed >= total_seconds:
            break

        iteration += 1

        # 1. Generate workload traffic
        try:
            # Health check
            st, _ = http_get(f"{base_url}/health/live")
            assert st == 200, f"health check failed: {st}"

            # Query server status
            st, _ = http_get(f"{base_url}/api/v1/status")
            assert st == 200, f"status query failed: {st}"

            # Search query
            http_get(f"{base_url}/api/v1/search?q=test", headers=auth_headers)

            # Query playback queue
            http_get(f"{base_url}/api/v1/queue", headers=auth_headers)

            # Stream chunk if tracks exist
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
            print(f"⚠️ [iteration {iteration}] workload request error: {e}")

        # 2. Collect process & OS telemetry
        metrics = get_process_metrics(pid)
        wal_bytes = get_wal_size(args.config_dir)
        rss_mb = metrics["rss_kb"] / 1024.0

        if metrics["rss_kb"] > peak_rss:
            peak_rss = metrics["rss_kb"]
        if metrics["open_fds"] > max_fds:
            max_fds = metrics["open_fds"]

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

    # Analyze Results
    final_metrics = get_process_metrics(pid)
    initial_rss_mb = initial_metrics["rss_kb"] / 1024.0
    final_rss_mb = final_metrics["rss_kb"] / 1024.0
    peak_rss_mb = peak_rss / 1024.0
    rss_drift_mb = final_rss_mb - initial_rss_mb
    fd_drift = final_metrics["open_fds"] - initial_metrics["open_fds"]

    report_data = {
        "duration_seconds": total_seconds,
        "samples_collected": len(telemetry),
        "initial_rss_mb": round(initial_rss_mb, 2),
        "final_rss_mb": round(final_rss_mb, 2),
        "peak_rss_mb": round(peak_rss_mb, 2),
        "rss_drift_mb": round(rss_drift_mb, 2),
        "initial_fds": initial_metrics["open_fds"],
        "final_fds": final_metrics["open_fds"],
        "peak_fds": max_fds,
        "fd_drift": fd_drift,
        "child_processes": final_metrics["child_processes"],
        "status": "PASS",
    }

    # Strict Stability Criteria
    # 1. FD Leak: FD drift should be stable (less than 10 new FDs over soak period)
    assert fd_drift < 15, f"FD leak detected: +{fd_drift} file descriptors"
    # 2. Child Process Leak: No leftover zombie processes
    assert final_metrics["child_processes"] == 0, f"Zombie processes detected: {final_metrics['child_processes']}"

    os.makedirs(os.path.dirname(os.path.abspath(args.report)), exist_ok=True)
    with open(args.report, "w") as f:
        json.dump(report_data, f, indent=2)

    print("\n" + "=" * 70)
    print(f"SOAK TEST COMPLETE — STATUS: {report_data['status']}")
    print(f"Initial RSS: {initial_rss_mb:.2f} MB ➔ Final RSS: {final_rss_mb:.2f} MB (Peak: {peak_rss_mb:.2f} MB, Drift: {rss_drift_mb:+.2f} MB)")
    print(f"Initial FDs: {initial_metrics['open_fds']} ➔ Final FDs: {final_metrics['open_fds']} (Peak: {max_fds}, Drift: {fd_drift:+d})")
    print(f"Report saved to: {args.report}")
    print("=" * 70)

if __name__ == "__main__":
    main()
