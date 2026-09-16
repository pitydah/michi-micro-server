#!/usr/bin/env python3
"""
Michi Micro Server - WebUI Action Contract Verification Script
Enforces semantic contract conformance:
1. Every action has an authoritative handler and valid authority.
2. Every action endpoint matches an Axum router route in crates/michi-api.
3. Every action test ID exists in the automated test suite.
4. All visible onclick/onchange HTML handlers map to manifest actions or allowed UI helpers.
5. No banned legacy endpoints in app.js or index.html.
6. No silent empty catches in app.js.
"""

import ast
import glob
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HTML_PATH = ROOT / "crates/michi-api/static/index.html"
JS_PATH = ROOT / "crates/michi-api/static/app.js"
MANIFEST_PATH = ROOT / "spec/v1/webui-actions.json"

REGISTRY_PATH = ROOT / "tests/webui/action_contract_registry.json"

BANNED_LEGACY_PATTERNS = [
    r"/api/status\b",
    r"/api/v1/episodes/",
    r"/api/playback/record\b",
]

ALLOWED_EMPTY_CATCHES = [
    "audio.pause",
    "Audio pause on teardown",
    "sw.js",
    "serviceWorker",
]

ALLOWED_UI_HELPERS = {
    "closeModal",
    "closeAuthModal",
    "closeChainDetail",
    "closeTrackDetailModal",
    "openAuthModal",
    "showCreateChain",
    "hideCreateChain",
    "showSection",
    "switchAuthTab",
    "switchPlaylistTab",
    "switchSettingsTab",
    "copyServerUrl",
    "event.stopPropagation",
    "stopPropagation",
    "setLanguage",
    "setTheme",
    "checkForUpdates",
    "checkUpdateStatus",
}


def load_manifest():
    if not MANIFEST_PATH.exists():
        print(f"❌ Error: Manifest missing at {MANIFEST_PATH}")
        sys.exit(1)
    with open(MANIFEST_PATH, "r", encoding="utf-8") as f:
        return json.load(f)


def load_registry():
    if not REGISTRY_PATH.exists():
        return None
    with open(REGISTRY_PATH, "r", encoding="utf-8") as f:
        return json.load(f)


def inspect_html():
    with open(HTML_PATH, "r", encoding="utf-8") as f:
        html = f.read()

    onclicks = set(re.findall(r'onclick=[\"\']([^\"\']+)[\"\']', html))
    onchanges = set(re.findall(r'onchange=[\"\']([^\"\']+)[\"\']', html))
    settings = set(re.findall(r'saveSetting\([\'\"]([^\'\"]+)[\'\"]', html))

    return {
        "html": html,
        "onclicks": onclicks,
        "onchanges": onchanges,
        "settings": settings,
    }


def extract_function_body(name, js_text):
    patterns = [
        rf"(?:async\s+)?function\s+{re.escape(name)}\s*\([^)]*\)\s*\{{",
        rf"\b{re.escape(name)}\s*:\s*(?:async\s+)?function\s*\([^)]*\)\s*\{{",
        rf"\b{re.escape(name)}\s*\([^)]*\)\s*\{{"
    ]
    for p in patterns:
        m = re.search(p, js_text)
        if m:
            start_idx = m.end() - 1
            depth = 1
            pos = start_idx + 1
            while pos < len(js_text) and depth > 0:
                if js_text[pos] == "{":
                    depth += 1
                elif js_text[pos] == "}":
                    depth -= 1
                pos += 1
            return js_text[start_idx:pos]
    return None


def inspect_js():
    with open(JS_PATH, "r", encoding="utf-8") as f:
        js = f.read()

    func_defs = set(re.findall(r'(?:async\s+function|function)\s+([a-zA-Z0-9_$]+)', js))
    method_defs = set(re.findall(r'\b([a-zA-Z0-9_$]+)\s*\([^)]*\)\s*\{', js))

    return {
        "js": js,
        "functions": func_defs | method_defs,
    }


def scan_axum_routes():
    """Extracts all registered Axum routes from crates/michi-api using balanced-parentheses parsing."""
    pattern = re.compile(r'\.route\s*\(\s*\"([^\"]+)\"\s*,')
    all_routes = set()

    for rs_file in glob.glob(str(ROOT / "crates/michi-api/src/**/*.rs"), recursive=True):
        with open(rs_file, "r", encoding="utf-8") as f:
            content = f.read()

        pos = 0
        while True:
            m = pattern.search(content, pos)
            if not m:
                break
            path = m.group(1)
            idx = m.end()
            depth = 1
            while idx < len(content) and depth > 0:
                if content[idx] == '(':
                    depth += 1
                elif content[idx] == ')':
                    depth -= 1
                idx += 1
            body = content[m.end():idx - 1]
            pos = idx

            norm_path = re.sub(r':\w+', ':param', path)
            norm_path = re.sub(r'\{[^}]+\}', ':param', norm_path)
            for method in ['get', 'post', 'put', 'delete', 'patch', 'head', 'options']:
                if re.search(r'\b' + method + r'\s*\(', body):
                    all_routes.add((method.upper(), norm_path))

    return all_routes


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
    empty_catch_pattern = re.compile(
        r'\.catch\s*\(\s*(?:function\s*\([^)]*\)\s*\{\s*\}|\([^)]*\)\s*=>\s*\{\s*\}|\(\)\s*=>\s*\{\s*\})\s*\)'
    )
    matches = empty_catch_pattern.finditer(js)
    for m in matches:
        snippet = js[max(0, m.start() - 30):min(len(js), m.end() + 30)].replace("\n", " ")
        if any(allowed in snippet for allowed in ALLOWED_EMPTY_CATCHES):
            continue
        line_num = js[:m.start()].count("\n") + 1
        violations.append(f"Line {line_num}: Unhandled silent catch: {snippet}")
    return violations


def check_html_handlers(html_data, actions):
    raw_handlers = set()
    for evt in html_data["onclicks"] | html_data["onchanges"]:
        for stmt in evt.split(';'):
            stmt = stmt.strip()
            if not stmt:
                continue
            m = re.match(r'([a-zA-Z0-9_$.]+)', stmt)
            if m:
                raw_handlers.add(m.group(1))

    manifest_handlers = set()
    for a in actions:
        fh = a.get("frontend_handler")
        if fh:
            manifest_handlers.add(fh)
            if '.' in fh:
                manifest_handlers.add(fh.split('.')[-1])

    violations = []
    for h in raw_handlers:
        if h not in manifest_handlers and h not in ALLOWED_UI_HELPERS:
            violations.append(f"Unaccounted HTML event handler '{h}' is neither in manifest nor allowed UI helpers")
    return violations


def main():
    print("======================================================================")
    print("MICHI MICRO SERVER — WEBUI ACTION CONTRACT VERIFIER")
    print("======================================================================")

    manifest = load_manifest()
    actions = manifest.get("actions", [])
    print(f"📋 Loaded {len(actions)} actions from {MANIFEST_PATH.name}")

    registry = load_registry()
    if registry:
        print(f"📑 Loaded test contract registry from {REGISTRY_PATH.name}")
    else:
        print(f"⚠️ Warning: Registry not found at {REGISTRY_PATH}")

    html_data = inspect_html()
    js_data = inspect_js()

    action_ids = set()
    errors = []

    # Verify bidirectional 1-to-1 parity between manifest and registry
    manifest_action_ids = set(a.get("id") for a in actions if a.get("id"))
    if registry and registry.get("actions"):
        reg_action_ids = set(registry["actions"].keys())
        missing_in_reg = manifest_action_ids - reg_action_ids
        if missing_in_reg:
            for m_id in sorted(missing_in_reg):
                errors.append(f"Action '{m_id}' defined in manifest but missing from registry {REGISTRY_PATH.name}")
        extra_in_reg = reg_action_ids - manifest_action_ids
        if extra_in_reg:
            for r_id in sorted(extra_in_reg):
                errors.append(f"Action '{r_id}' defined in registry {REGISTRY_PATH.name} but missing from manifest")

    # Collect test files corpus
    test_files = glob.glob(str(ROOT / "tests/**/*.py"), recursive=True) + glob.glob(
        str(ROOT / "crates/**/*.rs"), recursive=True
    )
    test_corpus = "\n".join(
        open(f, encoding="utf-8", errors="ignore").read() for f in test_files
    )

    # Collect Axum routes from backend
    axum_routes = scan_axum_routes()
    print(f"📡 Scanned {len(axum_routes)} active Axum routes from crates/michi-api")

    registry_verified_count = 0
    test_functions_verified_count = 0
    protected_guards_verified_count = 0
    conditional_guards_verified_count = 0

    # 1. Check each action in manifest
    for action in actions:
        aid = action.get("id")
        if not aid:
            errors.append("Manifest contains action without 'id'")
            continue
        if aid in action_ids:
            errors.append(f"Duplicate action id: {aid}")
        action_ids.add(aid)

        # Verify test id exists in test corpus
        test_id = action.get("test")
        if not test_id:
            errors.append(f"Action '{aid}' has no test reference")
        elif test_id not in test_corpus:
            errors.append(f"Action '{aid}' test reference '{test_id}' not found in any test file")

        # Verify against registry if present
        reg_action_has_error = False
        if registry and registry.get("actions"):
            reg_entry = registry["actions"].get(aid)
            if not reg_entry:
                errors.append(f"Action '{aid}' is not recorded in test contract registry {REGISTRY_PATH.name}")
                reg_action_has_error = True
            else:
                if reg_entry.get("test_id") != test_id:
                    errors.append(f"Action '{aid}' test_id mismatch between manifest ({test_id}) and registry ({reg_entry.get('test_id')})")
                    reg_action_has_error = True
                if reg_entry.get("auth") != action.get("auth"):
                    errors.append(f"Action '{aid}' auth mismatch between manifest ({action.get('auth')}) and registry ({reg_entry.get('auth')})")
                    reg_action_has_error = True
                if reg_entry.get("frontend_handler") != action.get("frontend_handler"):
                    errors.append(f"Action '{aid}' frontend_handler mismatch between manifest ({action.get('frontend_handler')}) and registry ({reg_entry.get('frontend_handler')})")
                    reg_action_has_error = True
                if action.get("auth") == "conditional":
                    m_cond = action.get("auth_condition")
                    r_cond = reg_entry.get("auth_condition")
                    if m_cond != r_cond:
                        errors.append(f"Action '{aid}' auth_condition mismatch between manifest ({m_cond}) and registry ({r_cond})")
                        reg_action_has_error = True
                else:
                    if reg_entry.get("auth_condition") is not None:
                        errors.append(f"Action '{aid}' non-conditional action has unexpected auth_condition in registry: {reg_entry.get('auth_condition')}")
                        reg_action_has_error = True
                test_fn = reg_entry.get("test_function")
                test_file = reg_entry.get("test_file")
                if not test_fn:
                    errors.append(f"Action '{aid}' registry entry missing test_function")
                    reg_action_has_error = True
                elif not test_file:
                    errors.append(f"Action '{aid}' registry entry missing test_file")
                    reg_action_has_error = True
                else:
                    tf_path = ROOT / test_file
                    if not tf_path.exists():
                        errors.append(f"Action '{aid}' registry test_file '{test_file}' does not exist")
                        reg_action_has_error = True
                    else:
                        try:
                            tf_content = tf_path.read_text(encoding="utf-8")
                            tree = ast.parse(tf_content, filename=str(tf_path))
                            found_fn_node = None
                            for node in ast.walk(tree):
                                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == test_fn:
                                    found_fn_node = node
                                    break
                            if not found_fn_node:
                                errors.append(f"Action '{aid}' test_function '{test_fn}' not found in AST of {test_file}")
                                reg_action_has_error = True
                            else:
                                fn_segment = ast.get_source_segment(tf_content, found_fn_node) or ""
                                if test_id not in fn_segment:
                                    errors.append(f"Action '{aid}' test_id '{test_id}' not found in source of {test_fn} ({test_file})")
                                    reg_action_has_error = True
                                else:
                                    test_functions_verified_count += 1
                        except Exception as e:
                            errors.append(f"Action '{aid}' failed to parse AST of {test_file}: {e}")
                            reg_action_has_error = True

                if not reg_action_has_error:
                    registry_verified_count += 1

        # Verify handler exists
        handler = action.get("frontend_handler")
        if handler and handler not in js_data["functions"]:
            if not any(f"{handler}" in js_data["js"] for h in [handler]):
                errors.append(f"Action '{aid}' references non-existent frontend_handler '{handler}'")

        # Verify authority
        auth_authority = action.get("authority")
        if auth_authority not in ("browser", "server", "shared", "local", "active_target"):
            errors.append(f"Action '{aid}' has invalid authority '{auth_authority}'")

        # Verify auth dimension: must be explicitly 'public', 'protected', 'conditional', or 'local_only'
        auth_level = action.get("auth")
        if not auth_level:
            errors.append(f"Action '{aid}' is missing required 'auth' field")
        elif auth_level not in ("public", "protected", "conditional", "local_only"):
            errors.append(f"Action '{aid}' has invalid auth value '{auth_level}' (expected 'public', 'protected', 'conditional', or 'local_only')")

        # Structural JS handler checks based on auth level
        if handler:
            fn_body = extract_function_body(handler, js_data["js"])
            if fn_body:
                if auth_level == "protected":
                    guard_pos = fn_body.find("canPerformProtectedAction()")
                    michi_pos = fn_body.find("MichiAPI.")
                    if guard_pos == -1:
                        errors.append(f"Action '{aid}' (protected) handler '{handler}' is missing 'canPerformProtectedAction()' guard")
                    elif michi_pos != -1 and guard_pos > michi_pos:
                        errors.append(f"Action '{aid}' (protected) handler '{handler}' calls MichiAPI before 'canPerformProtectedAction()' guard")
                    else:
                        protected_guards_verified_count += 1
                elif auth_level == "conditional":
                    cond = action.get("auth_condition")
                    cond_ok = True
                    if not cond or not str(cond).strip():
                        errors.append(f"Action '{aid}' is conditional but missing non-empty 'auth_condition' declaration")
                        cond_ok = False
                    michi_pos = fn_body.find("MichiAPI.")
                    if michi_pos != -1:
                        guard_pos = fn_body.find("canPerformProtectedAction()")
                        if guard_pos == -1:
                            errors.append(f"Action '{aid}' (conditional) handler '{handler}' calls MichiAPI but is missing 'canPerformProtectedAction()' guard")
                        elif guard_pos > michi_pos:
                            errors.append(f"Action '{aid}' (conditional) handler '{handler}' calls MichiAPI before 'canPerformProtectedAction()' guard")
                        elif cond_ok:
                            conditional_guards_verified_count += 1
                    elif cond_ok:
                        conditional_guards_verified_count += 1
                elif auth_level == "local_only":
                    if "MichiAPI." in fn_body:
                        errors.append(f"Action '{aid}' is local_only but handler '{handler}' makes MichiAPI network calls")

        # Verify endpoint matches Axum router
        ep = action.get("endpoint")
        if ep:
            m_method = ep.get("method", "").upper()
            m_path = ep.get("path", "")
            norm_m_path = re.sub(r':\w+', ':param', m_path)
            norm_m_path = re.sub(r'\{[^}]+\}', ':param', norm_m_path)
            if (m_method, norm_m_path) not in axum_routes:
                errors.append(
                    f"Action '{aid}' endpoint '{m_method} {m_path}' has no matching route in Axum router"
                )

    # 2. Check HTML event handlers
    html_violations = check_html_handlers(html_data, actions)
    for v in html_violations:
        errors.append(f"HTML_EVENT: {v}")

    # 3. Check banned legacy endpoints
    banned_violations = check_banned_endpoints(js_data["js"], html_data["html"])
    for v in banned_violations:
        errors.append(f"BANNED_ENDPOINT: {v}")

    # 4. Check empty / silent catches in JS
    catch_violations = check_empty_catches(js_data["js"])
    for v in catch_violations:
        errors.append(f"SILENT_CATCH: {v}")

    # Count auth categories
    public_count = sum(1 for a in actions if a.get("auth") == "public")
    protected_count = sum(1 for a in actions if a.get("auth") == "protected")
    conditional_count = sum(1 for a in actions if a.get("auth") == "conditional")
    local_only_count = sum(1 for a in actions if a.get("auth") == "local_only")

    # Output report
    report = {
        "actions_declared": len(actions),
        "actions_valid": len(action_ids) - len(errors),
        "public_actions": public_count,
        "protected_actions": protected_count,
        "conditional_actions": conditional_count,
        "local_only_actions": local_only_count,
        "registry_verified": registry_verified_count,
        "test_functions_verified": test_functions_verified_count,
        "protected_guards_verified": protected_guards_verified_count,
        "conditional_guards_verified": conditional_guards_verified_count,
        "errors": errors,
        "html_violations": html_violations,
        "banned_violations": banned_violations,
        "catch_violations": catch_violations,
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

    print("Action Contract Summary:")
    print(f"  actions_total: {len(actions)}")
    print(f"  public: {public_count}")
    print(f"  protected: {protected_count}")
    print(f"  conditional: {conditional_count}")
    print(f"  local_only: {local_only_count}")
    print(f"  registry_verified: {registry_verified_count}")
    print(f"  test_functions_verified: {test_functions_verified_count}")
    print(f"  protected_guards_verified: {protected_guards_verified_count}")
    print(f"  conditional_guards_verified: {conditional_guards_verified_count}")
    print("✅ WebUI action contract verification PASSED.")

    # Also enforce complete I18N locale parity
    i18n_script = Path(__file__).parent / "check_i18n_parity.py"
    if i18n_script.exists():
        import importlib.util
        spec = importlib.util.spec_from_file_location("check_i18n_parity", str(i18n_script))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        if not mod.verify_i18n_parity():
            return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
