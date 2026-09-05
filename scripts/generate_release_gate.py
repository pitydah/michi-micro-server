#!/usr/bin/env python3
"""
Evidence-Based Release Gate Aggregator and Report Generator for Michi Micro Server.

Reads requirement definitions from release/gates.json and evidence artifacts from target/release-evidence/.
Computes actual gate statuses without hardcoding, verifies commit provenance and evidence taxonomy.

Usage:
  python3 scripts/generate_release_gate.py [--mode rc|ga] [--requirements release/gates.json] [--evidence-dir target/release-evidence] [--report docs/V1_RELEASE_GATE.md] [--check]
"""

import argparse
import glob
import json
import os
import sys

from release_evidence import now_utc_iso, get_head_sha, validate_evidence_artifact

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def load_requirements(req_path: str):
    if not os.path.exists(req_path):
        print(f"ERROR: Requirements specification not found: {req_path}", file=sys.stderr)
        sys.exit(1)
    with open(req_path, "r", encoding="utf-8") as f:
        return json.load(f)

def load_evidence_artifacts(evidence_dir: str):
    artifacts = {}
    if not os.path.exists(evidence_dir):
        return artifacts
    
    for fpath in glob.glob(os.path.join(evidence_dir, "*.json")):
        try:
            with open(fpath, "r", encoding="utf-8") as f:
                data = json.load(f)
                gate_id = data.get("gate_id")
                if gate_id:
                    artifacts[gate_id] = data
        except Exception as e:
            print(f"WARNING: Failed to parse artifact {fpath}: {e}", file=sys.stderr)
    return artifacts

def aggregate_gates(reqs_data, artifacts, current_sha, mode):
    gates = reqs_data.get("gates", [])
    evaluated = []
    has_blocking_failure = False

    for g in gates:
        gid = g["id"]
        required_for = g.get("required_for", [])
        is_required = mode in required_for
        accepted_classes = g.get("accepted_evidence_classes", [])
        
        art = artifacts.get(gid)
        if not art:
            status = "NOT_RUN"
            ev_class = "NONE"
            detail = "No evidence artifact submitted for this gate"
        else:
            ev_class = art.get("evidence_class", "UNKNOWN")
            status, detail = validate_evidence_artifact(art, current_sha, accepted_classes)

        # Failure classification
        if is_required and status in {"FAIL", "INVALID_EVIDENCE", "STALE", "NOT_RUN"}:
            has_blocking_failure = True

        evaluated.append({
            "id": gid,
            "description": g.get("description", ""),
            "required": is_required,
            "required_for": required_for,
            "status": status,
            "evidence_class": ev_class,
            "detail": detail
        })

    return evaluated, has_blocking_failure

def status_icon(status: str) -> str:
    if status == "PASS":
        return "🟢 **PASS**"
    elif status == "FAIL":
        return "🔴 **FAIL**"
    elif status == "NOT_RUN":
        return "⚪ **NOT_RUN**"
    elif status == "BLOCKED_EXTERNAL":
        return "🟣 **BLOCKED_EXTERNAL**"
    elif status == "STALE":
        return "🟡 **STALE**"
    elif status == "INVALID_EVIDENCE":
        return "🟠 **INVALID_EVIDENCE**"
    return f"❓ **{status}**"

def generate_markdown(evaluated_gates, current_sha, mode, has_blocking_failure):
    now = now_utc_iso()
    overall = "🟢 **READY FOR RELEASE**" if not has_blocking_failure else "🔴 **RELEASE BLOCKED**"
    
    lines = [
        "# 🚪 Michi Micro Server — Release Gate v1.0.0 (Evidence Ledger)",
        "",
        f"- **Evaluated at:** `{now}`",
        f"- **Commit SHA:** `{current_sha}`",
        f"- **Evaluation Mode:** `{mode.upper()}`",
        f"- **Overall Decision:** {overall}",
        "",
        "---",
        "",
        "## 📊 Matriz Canónica de Requisitos y Evidencia Calculada",
        "",
        "| Gate ID | Requerido en Modo | Clase de Evidencia | Estado Calculado | Observaciones / Provenance |",
        "| :--- | :---: | :---: | :---: | :--- |",
    ]

    for g in evaluated_gates:
        req_str = f"`{mode.upper()}` (Mandatorio)" if g["required"] else "Opcional / Futuro"
        icon = status_icon(g["status"])
        lines.append(f"| **{g['id']}** | {req_str} | `{g['evidence_class']}` | {icon} | {g['detail']} |")

    lines.extend([
        "",
        "---",
        "",
        "## 📋 Principios de Certificación de Release",
        "1. **Sin estados hardcodeados**: Toda fila refleja la evaluación de un artifact generado durante la ejecución sobre el commit actual.",
        "2. **Cero tolerancia a STALE**: Evidencia generada para un commit diferente al HEAD actual es invalidada inmediatamente.",
        "3. **Taxonomía rígida**: Mocks o simuladores jamás son aceptados para satisfacer requisitos de integración real o hardware físico.",
        ""
    ])

    return "\n".join(lines)

def main():
    p = argparse.ArgumentParser(description="Release Gate Aggregator & Generator")
    p.add_argument("--mode", choices=["rc", "ga"], default="rc", help="Target release milestone (rc or ga)")
    p.add_argument("--requirements", default=os.path.join(ROOT_DIR, "release", "gates.json"))
    p.add_argument("--evidence-dir", default=os.path.join(ROOT_DIR, "target", "release-evidence"))
    p.add_argument("--report", default=os.path.join(ROOT_DIR, "docs", "V1_RELEASE_GATE.md"))
    p.add_argument("--check", action="store_true", help="Fail with non-zero exit code if required gates are blocked/failing")
    args = p.parse_args()

    sha = get_head_sha()
    reqs = load_requirements(args.requirements)
    artifacts = load_evidence_artifacts(args.evidence_dir)

    evaluated_gates, has_blocking_failure = aggregate_gates(reqs, artifacts, sha, args.mode)
    md_content = generate_markdown(evaluated_gates, sha, args.mode, has_blocking_failure)

    if args.report:
        report_path = os.path.abspath(args.report)
        os.makedirs(os.path.dirname(report_path), exist_ok=True)
        with open(report_path, "w", encoding="utf-8") as f:
            f.write(md_content)
        print(f"Generated release gate report -> {report_path}")

    # Summary console output
    print(f"\n[Release Gate Summary for {args.mode.upper()} @ {sha[:8]}]")
    for g in evaluated_gates:
        req_tag = "[REQ]" if g["required"] else "[OPT]"
        print(f"  {req_tag} {g['id']:<28} -> {g['status']:<16} ({g['detail']})")

    if args.check and has_blocking_failure:
        print(f"\n❌ RELEASE GATE CHECK FAILED: Mode '{args.mode}' has unsatisfied or failing required gates.", file=sys.stderr)
        sys.exit(1)
    elif args.check:
        print(f"\n✅ RELEASE GATE CHECK PASSED: Mode '{args.mode}' all required gates satisfied.")

if __name__ == "__main__":
    main()
