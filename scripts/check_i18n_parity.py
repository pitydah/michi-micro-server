#!/usr/bin/env python3
"""
I18N Locale Parity and Completeness Checker for Michi Micro Server.

Verifies that:
1. Canonical en.json contains all UI-referenced keys (data-i18n attributes and t('...') calls).
2. Every supported locale catalog contains all keys from en.json.
3. No translation in any locale is null, empty, or whitespace-only.
4. No translation silently falls back to the raw key name (e.g. "settings.rail_overview").
"""

import glob
import json
import os
import re
import sys

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STATIC_DIR = os.path.join(ROOT_DIR, "crates", "michi-api", "static")
I18N_DIR = os.path.join(STATIC_DIR, "i18n")

def extract_ui_referenced_keys():
    referenced = set()
    
    # 1. index.html data-i18n attributes
    html_path = os.path.join(STATIC_DIR, "index.html")
    if os.path.exists(html_path):
        with open(html_path, "r", encoding="utf-8") as f:
            html = f.read()
        for k in re.findall(r'data-i18n=[\x27\x22]([a-zA-Z0-9_.]+)[\x27\x22]', html):
            referenced.add(k)

    # 2. app.js t('...') calls
    app_js_path = os.path.join(STATIC_DIR, "app.js")
    if os.path.exists(app_js_path):
        with open(app_js_path, "r", encoding="utf-8") as f:
            js = f.read()
        for k in re.findall(r'\bt\(\s*[\x27\x22]([a-zA-Z0-9_.]+)[\x27\x22]', js):
            referenced.add(k)

    return referenced

def verify_i18n_parity():
    en_path = os.path.join(I18N_DIR, "en.json")
    if not os.path.exists(en_path):
        print(f"FAIL:\ncanonical en.json missing at {en_path}", file=sys.stderr)
        return False

    with open(en_path, "r", encoding="utf-8") as f:
        try:
            en = json.load(f)
        except Exception as e:
            print(f"FAIL:\ncanonical en.json invalid JSON: {e}", file=sys.stderr)
            return False

    failures = []

    # Verify UI referenced keys are in canonical en.json
    ui_keys = extract_ui_referenced_keys()
    for uk in sorted(ui_keys):
        if uk not in en:
            failures.append(f"en missing referenced UI key {uk}")

    # Discover all locale catalogs
    locale_files = sorted(glob.glob(os.path.join(I18N_DIR, "*.json")))
    if not locale_files:
        print(f"FAIL:\nno locale files found in {I18N_DIR}", file=sys.stderr)
        return False

    required_locales = {"en", "es", "de", "fr", "it", "ja", "pt", "ru", "zh"}
    found_locales = {os.path.splitext(os.path.basename(p))[0] for p in locale_files}
    missing_locales = required_locales - found_locales
    if missing_locales:
        for ml in sorted(missing_locales):
            failures.append(f"required locale catalog {ml}.json is missing")

    for loc_path in locale_files:
        loc_name = os.path.basename(loc_path)
        loc_code = os.path.splitext(loc_name)[0]

        with open(loc_path, "r", encoding="utf-8") as f:
            try:
                data = json.load(f)
            except Exception as e:
                failures.append(f"{loc_code} invalid JSON: {e}")
                continue

        for key, en_val in en.items():
            if key not in data:
                failures.append(f"{loc_code} missing {key}")
                continue

            val = data[key]
            if val is None:
                failures.append(f"{loc_code} null {key}")
            elif not isinstance(val, str) or not val.strip():
                failures.append(f"{loc_code} empty {key}")
            elif "." in key and val == key:
                failures.append(f"{loc_code} untranslated raw key {key}")

    if failures:
        print("FAIL:", file=sys.stderr)
        for fail in failures:
            print(f"  {fail}", file=sys.stderr)
        return False

    print(f"✅ I18n Parity: All {len(found_locales)} locale catalogs verified with 100% key parity ({len(en)} keys).")
    return True

def main():
    ok = verify_i18n_parity()
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    main()
