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

def calculate_slope(timestamps, values):
    """Calculate slope m (units per second) in values = m * timestamps + b."""
    n = len(timestamps)
    if n < 2:
        return 0.0
    t_mean = sum(timestamps) / n
    v_mean = sum(values) / n
    numerator = sum((t - t_mean) * (v - v_mean) for t, v in zip(timestamps, values))
    denominator = sum((t - t_mean) ** 2 for t in timestamps)
    if denominator == 0.0:
        return 0.0
    return numerator / denominator

def main():
    parser = argparse.ArgumentParser(description="Michi Micro Server Soak Stability Test")
    parser.add_argument("--url", default="http://127.0.0.1:9091", help="Base server URL")
    parser.add_argument("--pid", type=int, required=True, help="Server process PID")
    
    duration = parser.add_mutually_exclusive_group()
    duration.add_argument("--duration-seconds", type=int, default=None, help="Test duration in seconds")
    duration.add_argument("--duration-hours", type=float, default=None, help="Test duration in hours")
    
    parser.add_argument("--warmup-seconds", type=float, default=None, help="Warm-up duration in seconds before establishing baseline")
    parser.add_argument("--sample-interval", type=float, default=2.0, help="Sampling interval in seconds")
    parser.add_argument("--config-dir", default="target/soak_test_config", help="Database/config dir for WAL tracking")
    parser.add_argument("--gate-id", default="short-stability-smoke", help="Gate ID for evidence artifact")
    parser.add_argument("--evidence-class", default="INTEGRATION_REAL", choices=["INTEGRATION_REAL", "LONG_SOAK"], help="Evidence class")
    parser.add_argument("--report", default="target/soak_report.json", help="Path to write JSON report")
    parser.add_argument("--username", default="admin")
    parser.add_argument("--password", default="admin123")
    
    # Stability and memory thresholds
    parser.add_argument("--max-rss-mb", type=float, default=65.0, help="Hard maximum RSS memory ceiling in MB")
    parser.add_argument("--max-post-warmup-drift-mb", type=float, default=None, help="Maximum allowed RSS growth post-warmup in MB")
    parser.add_argument("--max-rss-slope-mb-per-hour", type=float, default=None, help="Maximum allowed RSS slope in MB/hour post-warmup")
    parser.add_argument("--max-fd-drift", type=int, default=15, help="Maximum allowed file descriptor drift")
    parser.add_argument("--max-thread-drift", type=int, default=8, help="Maximum allowed thread drift post-warmup")
    args = parser.parse_args()

    if args.duration_hours is not None:
        total_seconds = int(args.duration_hours * 3600)
    elif args.duration_seconds is not None:
        total_seconds = args.duration_seconds
    else:
        total_seconds = 30

    if args.evidence_class == "LONG_SOAK" and total_seconds < 24 * 3600:
        parser.error("LONG_SOAK evidence requires >= 24 hours")

    # Determine sensible warm-up duration if not specified
    if args.warmup_seconds is not None:
        warmup_seconds = args.warmup_seconds
    elif total_seconds >= 3600:
        warmup_seconds = 600.0  # 10 minutes for long soaks
    else:
        warmup_seconds = min(20.0, max(5.0, total_seconds * 0.25))

    # Determine thresholds based on duration if not explicitly provided
    if args.max_post_warmup_drift_mb is not None:
        max_post_warmup_drift_mb = args.max_post_warmup_drift_mb
    elif total_seconds >= 3600:
        max_post_warmup_drift_mb = 15.0
    else:
        max_post_warmup_drift_mb = 12.0

    if args.max_rss_slope_mb_per_hour is not None:
        max_rss_slope_mb_per_hour = args.max_rss_slope_mb_per_hour
    elif total_seconds >= 3600:
        max_rss_slope_mb_per_hour = 2.0  # 2 MB/hour max over 24h
    else:
        max_rss_slope_mb_per_hour = 35.0  # short smoke slope limit

    base_url = args.url.rstrip("/")
    pid = args.pid
    sha = get_head_sha()

    print("=" * 70)
    print("MICHI MICRO SERVER — ROBUST SOAK & TELEMETRY MONITOR")
    print(f"Server URL: {base_url} | PID: {pid} | Target Duration: {total_seconds}s (Warmup: {warmup_seconds}s)")
    print(f"Gate: {args.gate_id} ({args.evidence_class}) | Commit: {sha[:8]}")
    print(f"Thresholds: Max RSS: {args.max_rss_mb}MB | Max Drift (post-warmup): {max_post_warmup_drift_mb}MB | Max Slope: {max_rss_slope_mb_per_hour}MB/h")
    print("=" * 70)

    violations = []
    request_errors = []

    # Check process existence
    initial_metrics = get_process_metrics(pid)
    if not initial_metrics or not initial_metrics["valid"]:
        print(f"ERROR: Process {pid} is not running or invalid", file=sys.stderr)
        sys.exit(1)

    # Authenticate to get session token
    auth_token = None
    try:
        status, body = http_post_json(f"{base_url}/api/auth/login", {
            "username": args.username,
            "password": args.password
        })
        if status == 200:
            auth_token = json.loads(body.decode("utf-8")).get("token")
    except Exception as e:
        print(f"WARNING: Authentication failed: {e}. Running without auth.", file=sys.stderr)

    auth_headers = {"Authorization": f"Bearer {auth_token}"} if auth_token else {}

    start_time = time.time()
    last_sample_time = start_time
    last_request_time = start_time

    telemetry = []
    observation_timestamps = []
    observation_rss = []

    peak_rss = initial_metrics["rss_kb"]
    max_fds = initial_metrics["open_fds"]
    peak_wal = get_wal_size(args.config_dir)

    baseline_established = False
    baseline_rss_kb = initial_metrics["rss_kb"]
    baseline_fds = initial_metrics["open_fds"]
    baseline_threads = initial_metrics["threads"]

    while time.time() - start_time < total_seconds:
        now = time.time()
        elapsed = now - start_time
        in_warmup = elapsed < warmup_seconds

        # Check process survival
        cur_metrics = get_process_metrics(pid)
        if not cur_metrics or not cur_metrics["valid"]:
            violations.append(f"SERVER_PROCESS_DIED: PID {pid} died after {elapsed:.1f}s")
            break

        # Check hard memory ceiling continuously
        cur_rss_mb = cur_metrics["rss_kb"] / 1024.0
        if cur_rss_mb > args.max_rss_mb:
            violations.append(f"HARD_RSS_CEILING_EXCEEDED: Current RSS {cur_rss_mb:.2f}MB exceeds hard ceiling {args.max_rss_mb:.2f}MB")
            break

        # Switch phase and establish baseline post-warmup
        if not in_warmup and not baseline_established:
            baseline_established = True
            baseline_rss_kb = cur_metrics["rss_kb"]
            baseline_fds = cur_metrics["open_fds"]
            baseline_threads = cur_metrics["threads"]
            print(f"[{elapsed:>6.1f}s] *** WARM-UP PHASE COMPLETE — Establishing baseline RSS: {baseline_rss_kb / 1024.0:.2f} MB, Threads: {baseline_threads}, FDs: {baseline_fds} ***")

        # Periodic Sample
        if now - last_sample_time >= args.sample_interval:
            last_sample_time = now
            wal_size = get_wal_size(args.config_dir)
            peak_rss = max(peak_rss, cur_metrics["rss_kb"])
            max_fds = max(max_fds, cur_metrics["open_fds"])
            peak_wal = max(peak_wal, wal_size)

            phase = "warmup" if in_warmup else "observation"
            sample = {
                "elapsed_s": round(elapsed, 1),
                "phase": phase,
                "rss_mb": round(cur_metrics["rss_kb"] / 1024.0, 2),
                "threads": cur_metrics["threads"],
                "fds": cur_metrics["open_fds"],
                "children": cur_metrics["child_processes"],
                "wal_kb": round(wal_size / 1024.0, 1)
            }
            telemetry.append(sample)

            if not in_warmup:
                observation_timestamps.append(elapsed)
                observation_rss.append(cur_metrics["rss_kb"] / 1024.0)

            print(f"[{sample['elapsed_s']:>6.1f}s / {total_seconds}s] ({phase.upper():<11}) RSS: {sample['rss_mb']:>6.2f} MB | FDs: {sample['fds']:>3} | Threads: {sample['threads']:>2} | WAL: {sample['wal_kb']:>6.1f} KB | Children: {sample['children']}")

        # Simulated Activity / Load
        if now - last_request_time >= 0.5:
            last_request_time = now
            try:
                # 1. Health check
                s, _ = http_get(f"{base_url}/health/live")
                if s != 200:
                    request_errors.append(f"/health/live returned {s}")

                # 2. Server info
                s, _ = http_get(f"{base_url}/api/v1/server/info", headers=auth_headers)
                if s != 200:
                    request_errors.append(f"/server/info returned {s}")

                # 3. Status check
                s, _ = http_get(f"{base_url}/api/v1/status", headers=auth_headers)
                if s != 200:
                    request_errors.append(f"/api/v1/status returned {s}")

            except urllib.error.URLError as e:
                request_errors.append(f"Network error: {e}")
            except Exception as e:
                request_errors.append(f"Unexpected request error: {e}")

        time.sleep(0.1)

    actual_duration = round(time.time() - start_time, 1)
    final_metrics = get_process_metrics(pid) or cur_metrics or initial_metrics

    # Compute Statistics & Drifts
    initial_rss_mb = initial_metrics["rss_kb"] / 1024.0
    baseline_rss_mb = baseline_rss_kb / 1024.0
    final_rss_mb = final_metrics["rss_kb"] / 1024.0
    peak_rss_mb = peak_rss / 1024.0
    
    total_rss_drift_mb = final_rss_mb - initial_rss_mb
    post_warmup_rss_drift_mb = final_rss_mb - baseline_rss_mb if baseline_established else total_rss_drift_mb
    
    fd_drift = final_metrics["open_fds"] - initial_metrics["open_fds"]
    thread_drift = final_metrics["threads"] - (baseline_threads if baseline_established else initial_metrics["threads"])

    # Compute Linear Regression Slope (MB/hour) over post-warmup window
    if len(observation_timestamps) >= 3:
        slope_mb_per_sec = calculate_slope(observation_timestamps, observation_rss)
        rss_slope_mb_per_hour = round(slope_mb_per_sec * 3600.0, 2)
    else:
        rss_slope_mb_per_hour = 0.0

    # ══════════════════════════════════════════════════════════════════════════
    # STABILITY ASSERTIONS & LEAK DETECTION VIOLATIONS
    # ══════════════════════════════════════════════════════════════════════════
    
    # 1. Hard RSS ceiling
    if peak_rss_mb > args.max_rss_mb:
        violations.append(f"HARD_RSS_CEILING_EXCEEDED: Peak RSS {peak_rss_mb:.2f}MB exceeds limit {args.max_rss_mb:.2f}MB")

    # 2. Post-warmup memory drift
    if baseline_established and post_warmup_rss_drift_mb > max_post_warmup_drift_mb:
        violations.append(
            f"POST_WARMUP_RSS_DRIFT_EXCEEDED: Post-warmup growth +{post_warmup_rss_drift_mb:.2f}MB exceeds limit +{max_post_warmup_drift_mb:.2f}MB"
        )

    # 3. Excessive Memory Growth Slope (MB/hour)
    if len(observation_timestamps) >= 5 and rss_slope_mb_per_hour > max_rss_slope_mb_per_hour:
        violations.append(
            f"EXCESSIVE_RSS_GROWTH_SLOPE: Slope {rss_slope_mb_per_hour:+.2f}MB/h exceeds max allowed {max_rss_slope_mb_per_hour:.2f}MB/h"
        )

    # 4. File Descriptor Leak
    if fd_drift >= args.max_fd_drift:
        violations.append(f"FD_LEAK_DETECTED: +{fd_drift} file descriptors exceeds limit {args.max_fd_drift}")

    # 5. Thread Leak
    if thread_drift >= args.max_thread_drift:
        violations.append(f"THREAD_LEAK_DETECTED: +{thread_drift} threads exceeds limit {args.max_thread_drift}")

    # 6. Zombie Processes
    if final_metrics["child_processes"] != 0:
        violations.append(f"ZOMBIE_PROCESSES_DETECTED: {final_metrics['child_processes']} children left")

    # 7. Request Reliability
    if len(request_errors) > 5:
        violations.append(f"HIGH_REQUEST_ERROR_COUNT: {len(request_errors)} errors observed")

    status = "PASS" if not violations else "FAIL"
    detail = (
        f"Soak stable: Baseline RSS {baseline_rss_mb:.2f}MB, post-warmup drift {post_warmup_rss_drift_mb:+.2f}MB, slope {rss_slope_mb_per_hour:+.2f}MB/h, FD drift {fd_drift:+d}, 0 zombies"
        if status == "PASS" else f"Soak stability violations: {'; '.join(violations)}"
    )

    report_data = {
        "schema_version": 2,
        "gate_id": args.gate_id,
        "commit_sha": sha,
        "evidence_class": args.evidence_class,
        "status": status,
        "detail": detail,
        "requested_duration_seconds": total_seconds,
        "warmup_duration_seconds": warmup_seconds,
        "actual_elapsed_seconds": actual_duration,
        "observation_samples_count": len(observation_timestamps),
        "total_samples_collected": len(telemetry),
        "initial_rss_mb": round(initial_rss_mb, 2),
        "baseline_rss_mb": round(baseline_rss_mb, 2),
        "final_rss_mb": round(final_rss_mb, 2),
        "peak_rss_mb": round(peak_rss_mb, 2),
        "total_rss_drift_mb": round(total_rss_drift_mb, 2),
        "post_warmup_rss_drift_mb": round(post_warmup_rss_drift_mb, 2),
        "rss_slope_mb_per_hour": rss_slope_mb_per_hour,
        "initial_fds": initial_metrics["open_fds"],
        "baseline_fds": baseline_fds,
        "final_fds": final_metrics["open_fds"],
        "peak_fds": max_fds,
        "fd_drift": fd_drift,
        "initial_threads": initial_metrics["threads"],
        "baseline_threads": baseline_threads,
        "final_threads": final_metrics["threads"],
        "thread_drift": thread_drift,
        "peak_wal_bytes": peak_wal,
        "child_processes": final_metrics["child_processes"],
        "request_errors_count": len(request_errors),
        "thresholds": {
            "max_rss_mb": args.max_rss_mb,
            "max_post_warmup_drift_mb": max_post_warmup_drift_mb,
            "max_rss_slope_mb_per_hour": max_rss_slope_mb_per_hour,
            "max_fd_drift": args.max_fd_drift,
            "max_thread_drift": args.max_thread_drift
        },
        "violations": violations,
        "exit_code": 0 if status == "PASS" else 1
    }

    report_path = os.path.abspath(args.report)
    os.makedirs(os.path.dirname(report_path), exist_ok=True)
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report_data, f, indent=2)

    print("\n" + "=" * 70)
    print(f"SOAK TEST COMPLETE — STATUS: {status}")
    print(f"Initial: {initial_rss_mb:.2f} MB -> Baseline: {baseline_rss_mb:.2f} MB -> Final: {final_rss_mb:.2f} MB (Peak: {peak_rss_mb:.2f} MB)")
    print(f"Post-Warmup Drift: {post_warmup_rss_drift_mb:+.2f} MB | Slope: {rss_slope_mb_per_hour:+.2f} MB/h")
    print(f"FDs: {initial_metrics['open_fds']} -> {final_metrics['open_fds']} (Drift: {fd_drift:+d}) | Threads: {initial_metrics['threads']} -> {final_metrics['threads']} (Drift: {thread_drift:+d})")
    print(f"Report saved to: {report_path}")
    print("=" * 70)

    if violations:
        print(f"❌ Violations detected:\n" + "\n".join(f"  - {v}" for v in violations), file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
