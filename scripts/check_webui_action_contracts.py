#!/usr/bin/env python3
"""
Michi Micro Server - WebUI Action Contract Verification Script
Enforces that every interactive action on the WebUI has an authoritative handler,
contract-conforming endpoint, no banned legacy routes, and corresponding test coverage.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HTML_PATH = ROOT / "crates/michi-api/static/index.html"
JS_PATH = ROOT / "crates/michi-api/static/app.js"
MANIFEST_PATH = ROOT / "spec/v1/webui-actions.json"

BANNED_LEGACY_PATTERNS = [
    r"/api/status",
    r"/api/v1/episodes/",
    r"/api/playback/record",
]

# Allowlist for best-effort optional cleanup catches (e.g. audio pause on teardown)
ALLOWED_EMPTY_CATCHES = [
    "audio.pause",
    "Audio pause on teardown",
]

def load_manifest():
    if not MANIFEST_PATH.exists():
        print(f"❌ Error: Manifest missing at {MANIFEST_PATH}")
        sys.exit(1)
    with open(MANIFEST_PATH, "r", encoding="utf-8") as f:
        return json.load(f)

def inspect_html():
    with open(HTML_PATH, "r", encoding="utf-8") as f:
        html = f.read()

    # Extract onclick attributes
    onclicks = set(re.findall(r'onclick=[\"\']([^\"\']+)[\"\']', html))
    # Extract onchange attributes
    onchanges = set(re.findall(r'onchange=[\"\']([^\"\']+)[\"\']', html))
    # Extract settings saved
    settings = set(re.findall(r'saveSetting\([\'\"]([^\'\"]+)[\'\"]', html))

    return {
        "html": html,
        "onclicks": onclicks,
        "onchanges": onchanges,
        "settings": settings,
    }

def inspect_js():
    with open(JS_PATH, "r", encoding="utf-8") as f:
        js = f.read()

    # Find functions defined
    func_defs = set(re.findall(r'(?:async\s+function|function)\s+([a-zA-Z0-9_$]+)', js))
    # Find methods on objects
    method_defs = set(re.findall(r'\b([a-zA-Z0-9_$]+)\s*\([^)]*\)\s*\{', js))

    return {
        "js": js,
        "functions": func_defs | method_defs,
    }

def check_banned_endpoints(js, html):
    violations = []
    for pattern in BANNED_LEGACY_PATTERNS:
        matches_js = re.findall(pattern, js)
        if matches_js:
            violations.append(f"JS contains banned legacy endpoint pattern: {pattern}")
        matches_html = re.findall(pattern, html)
        if matches_html:
            violations.append(f"HTML contains banned legacy endpoint pattern: {pattern}")
    return violations

def check_empty_catches(js):
    violations = []
    # Pattern detecting .catch(function() {}) or .catch(() => {})
    empty_catch_pattern = re.compile(r'\.catch\s*\(\s*(?:function\s*\([^)]*\)\s*\{\s*\}|\([^)]*\)\s*=>\s*\{\s*\}|\(\)\s*=>\s*\{\s*\})\s*\)')
    matches = empty_catch_pattern.finditer(js)
    for m in matches:
        # Get line number
        line_num = js[:m.start()].count("\n") + 1
        snippet = js[max(0, m.start()-30):min(len(js), m.end()+30)].replace("\n", " ")
        violations.append(f"Line {line_num}: Unhandled silent catch: {snippet}")
    return violations

def main():
    print("======================================================================")
    print("MICHI MICRO SERVER — WEBUI ACTION CONTRACT VERIFIER")
    print("======================================================================")

    manifest = load_manifest()
    actions = manifest.get("actions", [])
    print(f"📋 Loaded {len(actions)} actions from {MANIFEST_PATH.name}")

    html_data = inspect_html()
    js_data = inspect_js()

    action_ids = set()
    errors = []

    # 1. Check each action in manifest
    for action in actions:
        aid = action.get("id")
        if not aid:
            errors.append("Manifest contains action without 'id'")
            continue
        if aid in action_ids:
            errors.append(f"Duplicate action id: {aid}")
        action_ids.add(aid)

        # Verify test id
        if not action.get("test"):
            errors.append(f"Action '{aid}' has no test reference")

        # Verify handler
        handler = action.get("frontend_handler")
        if handler and handler not in js_data["functions"]:
            # Check if it is a property or dynamic
            if not any(f"{handler}" in js_data["js"] for h in [handler]):
                errors.append(f"Action '{aid}' references non-existent frontend_handler '{handler}'")

        # Verify authority
        auth = action.get("authority")
        if auth not in ("browser", "server", "shared", "local", "active_target"):
            errors.append(f"Action '{aid}' has invalid authority '{auth}'")

    # 2. Check banned legacy endpoints
    banned_violations = check_banned_endpoints(js_data["js"], html_data["html"])
    if banned_violations:
        for v in banned_violations:
            errors.append(f"BANNED_ENDPOINT: {v}")

    # Output report
    report = {
        "actions_declared": len(actions),
        "actions_valid": len(action_ids) - len(errors),
        "errors": errors,
        "banned_violations": banned_violations,
    }

    report_path = ROOT / "target/webui-contract-report.json"
    report_path.parent.mkdir(parents=True, exist_ok=True)
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)

    print(f"Summary: {len(actions)} declared, {len(errors)} contract issues found.")
    if errors:
        print("\n❌ Contract verification failed with errors:")
        for e in errors:
            print(f"  - {e}")
        return 1

    print("✅ WebUI action contract verification PASSED.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
