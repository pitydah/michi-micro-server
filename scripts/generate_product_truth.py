#!/usr/bin/env python3
"""
Generates and checks Product Truth documentation across README.md, PRODUCT.md,
docs/ROADMAP.md and crates/michi-api/src/server_caps.rs.

Usage:
  python3 scripts/generate_product_truth.py [--check] [--write]
"""

import argparse
import json
import os
import re
import sys

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def load_product_truth():
    spec_path = os.path.join(ROOT_DIR, "spec", "v1", "product-truth.json")
    with open(spec_path, "r", encoding="utf-8") as f:
        return json.load(f)

def generate_matrix_markdown(spec):
    lines = [
        "| Feature | Release Scope | Maturity | Description |",
        "| :--- | :---: | :---: | :--- |",
    ]
    features = spec.get("features", {})
    for fid, finfo in sorted(features.items()):
        mat = finfo.get("maturity", "unknown").upper()
        if mat == "STABLE":
            mat_str = "🟢 `stable`"
        elif mat == "BETA":
            mat_str = "🟡 `beta`"
        else:
            mat_str = "⚪ `unavailable`"
        scope = f"`{finfo.get('release_scope')}`"
        desc = finfo.get("description", "")
        lines.append(f"| **{fid}** | {scope} | {mat_str} | {desc} |")
    return "\n".join(lines)

def update_file_matrix(filepath, matrix_md, check=False):
    if not os.path.exists(filepath):
        print(f"❌ File not found: {filepath}", file=sys.stderr)
        return False
    with open(filepath, "r", encoding="utf-8") as f:
        content = f.read()

    pattern = r"(<!-- BEGIN GENERATED V1 FEATURE MATRIX -->\n)(.*?)(\n<!-- END GENERATED V1 FEATURE MATRIX -->)"
    match = re.search(pattern, content, flags=re.DOTALL)
    if not match:
        print(f"❌ Missing Product Truth markers in {filepath}", file=sys.stderr)
        return False

    new_content = re.sub(pattern, f"\\1{matrix_md}\\3", content, flags=re.DOTALL)
    if check:
        if new_content != content:
            print(f"❌ Product Truth drift detected in {filepath}", file=sys.stderr)
            return False
        return True
    else:
        with open(filepath, "w", encoding="utf-8") as f:
            f.write(new_content)
        print(f"✓ Updated product matrix in {filepath}")
        return True

def generate_server_caps_block(spec):
    features = spec.get("features", {})
    mapping = {
        "stable": "FeatureMaturity::Stable",
        "beta": "FeatureMaturity::Beta",
        "unavailable": "FeatureMaturity::Unavailable",
    }
    entries = []
    for fid, finfo in sorted(features.items()):
        mat = mapping.get(finfo.get("maturity", "").lower(), "FeatureMaturity::Unavailable")
        entries.append(f'    ("{fid}", {mat}),')
    inner = "\n".join(entries)
    return f"// BEGIN GENERATED PRODUCT MATURITY\nconst CANONICAL_MATURITY: &[(&str, FeatureMaturity)] = &[\n{inner}\n];\n// END GENERATED PRODUCT MATURITY"

def update_server_caps(filepath, caps_block, check=False):
    if not os.path.exists(filepath):
        print(f"❌ File not found: {filepath}", file=sys.stderr)
        return False
    with open(filepath, "r", encoding="utf-8") as f:
        content = f.read()

    pattern = r"(// BEGIN GENERATED PRODUCT MATURITY\n)(.*?)(\n// END GENERATED PRODUCT MATURITY)"
    match = re.search(pattern, content, flags=re.DOTALL)
    if not match:
        print(f"❌ Missing Canonical Maturity markers in {filepath}", file=sys.stderr)
        return False

    new_content = re.sub(pattern, caps_block, content, flags=re.DOTALL)
    if check:
        if new_content != content:
            print(f"❌ Canonical Maturity drift detected in {filepath}", file=sys.stderr)
            return False
        return True
    else:
        with open(filepath, "w", encoding="utf-8") as f:
            f.write(new_content)
        print(f"✓ Updated canonical maturity in {filepath}")
        return True

def main():
    p = argparse.ArgumentParser(description="Product Truth Generator & Validator")
    p.add_argument("--check", action="store_true", help="Check for documentation drift")
    p.add_argument("--write", action="store_true", help="Write changes to files")
    args = p.parse_args()

    spec = load_product_truth()
    matrix_md = generate_matrix_markdown(spec)
    caps_block = generate_server_caps_block(spec)

    target_files = [
        os.path.join(ROOT_DIR, "README.md"),
        os.path.join(ROOT_DIR, "PRODUCT.md"),
        os.path.join(ROOT_DIR, "docs", "ROADMAP.md")
    ]
    caps_file = os.path.join(ROOT_DIR, "crates", "michi-api", "src", "server_caps.rs")

    all_ok = True
    for tf in target_files:
        ok = update_file_matrix(tf, matrix_md, check=args.check)
        if not ok:
            all_ok = False

    ok_caps = update_server_caps(caps_file, caps_block, check=args.check)
    if not ok_caps:
        all_ok = False

    if args.check:
        if all_ok:
            print("✅ Product Truth: All feature matrices and runtime projection are consistent.")
            sys.exit(0)
        else:
            print("❌ Product Truth: Documentation/Runtime is out of sync with spec/v1/product-truth.json", file=sys.stderr)
            sys.exit(1)

if __name__ == "__main__":
    main()
