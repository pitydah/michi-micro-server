#!/usr/bin/env python3
"""
Executes a gate command and records structured release evidence.

Usage:
  python3 scripts/run_gate.py --gate <gate_id> --class <evidence_class> --output <artifact_path> [--detail <detail>] -- <command...>
"""

import argparse
import json
import os
import subprocess
import sys

from release_evidence import now_utc_iso, get_head_sha, VALID_EVIDENCE_CLASSES

def main():
    p = argparse.ArgumentParser(description="Run command and record release evidence")
    p.add_argument("--gate", required=True, help="Gate identifier matching release/gates.json")
    p.add_argument("--class", dest="evidence_class", required=True, choices=sorted(VALID_EVIDENCE_CLASSES))
    p.add_argument("--output", required=True, help="Output path for JSON evidence artifact")
    p.add_argument("--detail", default="", help="Optional summary detail")
    p.add_argument("command", nargs=argparse.REMAINDER, help="Command to execute")
    
    args = p.parse_args()
    cmd = args.command
    if cmd and cmd[0] == "--":
        cmd = cmd[1:]
    if not cmd:
        p.error("Command is required")

    sha = get_head_sha()
    started_at = now_utc_iso()
    print(f"==> Running Gate [{args.gate}] (Class: {args.evidence_class}) at {sha[:8]}...")
    print(f"    Command: {' '.join(cmd)}")
    
    proc = subprocess.run(cmd)
    finished_at = now_utc_iso()
    
    status = "PASS" if proc.returncode == 0 else "FAIL"
    detail = args.detail or ("Command succeeded" if proc.returncode == 0 else f"Command failed with exit code {proc.returncode}")

    artifact = {
        "schema_version": 1,
        "gate_id": args.gate,
        "commit_sha": sha,
        "evidence_class": args.evidence_class,
        "status": status,
        "detail": detail,
        "command": cmd,
        "started_at": started_at,
        "finished_at": finished_at,
        "exit_code": proc.returncode,
        "environment": {
            "github_run_id": os.getenv("GITHUB_RUN_ID"),
            "github_job": os.getenv("GITHUB_JOB"),
            "ci": bool(os.getenv("CI")),
            "arch": os.uname().machine if hasattr(os, "uname") else "unknown"
        }
    }

    output_path = os.path.abspath(args.output)
    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(artifact, f, indent=2)

    print(f"==> Recorded evidence artifact for [{args.gate}] -> {output_path} (Status: {status})")
    sys.exit(proc.returncode)

if __name__ == "__main__":
    main()
