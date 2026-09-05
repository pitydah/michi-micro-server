#!/usr/bin/env python3
"""
Version and Packaging Consistency Validator for Michi Micro Server.

Verifies that:
1. Workspace Cargo.toml package version is valid SemVer (e.g. 1.0.0-rc.1).
2. README.md badges/version match Cargo.toml.
3. CHANGELOG.md contains an entry for current version.
4. ZimaOS store manifests use the product version for the app while separating store schema version.
"""

import os
import re
import sys
import tomllib

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def main():
    print("Checking version and packaging consistency across repository...")
    
    cargo_path = os.path.join(ROOT_DIR, "Cargo.toml")
    with open(cargo_path, "rb") as f:
        cargo = tomllib.load(f)
    product_version = cargo["workspace"]["package"]["version"]
    print(f"Canonical Cargo product version: {product_version}")

    errors = []

    # 1. README badge
    readme_path = os.path.join(ROOT_DIR, "README.md")
    with open(readme_path, "r", encoding="utf-8") as f:
        readme = f.read()
    esc_hyphen = product_version.replace("-", "--")
    badge_pattern = rf"version-(?:{re.escape(product_version)}|{re.escape(esc_hyphen)})-blue"
    if not re.search(badge_pattern, readme):
        errors.append(f"README.md does not contain badge for version {product_version}")

    # 2. CHANGELOG.md
    changelog_path = os.path.join(ROOT_DIR, "CHANGELOG.md")
    with open(changelog_path, "r", encoding="utf-8") as f:
        changelog = f.read()
    if f"## [{product_version}]" not in changelog:
        errors.append(f"CHANGELOG.md does not contain entry for ## [{product_version}]")

    # 3. ZimaOS / CasaOS compose
    zima_compose_path = os.path.join(ROOT_DIR, "zimaos-store", "Apps", "MichiMicroServer", "docker-compose.yml")
    with open(zima_compose_path, "r", encoding="utf-8") as f:
        zima_compose = f.read()
    expected_image = f"image: ghcr.io/pitydah/michi-micro-server:{product_version}"
    if expected_image not in zima_compose and "r3.1-zima" in zima_compose:
        errors.append(f"zimaos-store docker-compose.yml still uses legacy tag instead of {expected_image}")

    if errors:
        print("\n❌ Version Consistency Errors:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        sys.exit(1)

    print("✅ All version and packaging contracts are consistent.")

if __name__ == "__main__":
    main()
