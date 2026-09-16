#!/usr/bin/env python3
"""
Version and Packaging Consistency Validator for Michi Micro Server.

Verifies that:
1. Workspace Cargo.toml package version is valid strict SemVer (e.g. 1.0.0-rc.1).
2. README.md badges/version match Cargo.toml.
3. CHANGELOG.md contains an entry for current version.
4. Compose files use exact container image tag matching product version via safe YAML parsing.
"""

import os
import re
import sys
import tomllib
import yaml

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

SEMVER_RE = re.compile(
    r"^(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)"
    r"(?:-"
    r"(?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*"
    r")?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)

def main():
    print("Checking version and packaging consistency across repository...")
    
    cargo_path = os.path.join(ROOT_DIR, "Cargo.toml")
    with open(cargo_path, "rb") as f:
        cargo = tomllib.load(f)
    product_version = cargo["workspace"]["package"]["version"]
    print(f"Canonical Cargo product version: {product_version}")

    errors = []

    # 0. Strict SemVer check
    if not SEMVER_RE.fullmatch(product_version):
        errors.append(f"Workspace version is not valid SemVer: {product_version}")

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

    # 3. Safe YAML compose check & deployment platform verification
    expected_image = f"ghcr.io/pitydah/michi-micro-server:{product_version}"
    compose_targets = [
        (os.path.join(ROOT_DIR, "docker-compose.yml"), "docker"),
        (os.path.join(ROOT_DIR, "zimaos-store", "Apps", "MichiMicroServer", "docker-compose.yml"), "zimaos"),
        (os.path.join(ROOT_DIR, "casaos", "docker-compose.zimaos.yml"), "zimaos"),
        (os.path.join(ROOT_DIR, "casaos", "docker-compose.casaos.yml"), "casaos"),
    ]

    for comp_path, expected_platform in compose_targets:
        if os.path.exists(comp_path):
            with open(comp_path, "r", encoding="utf-8") as f:
                compose = yaml.safe_load(f)
            services = compose.get("services", {})
            svc = services.get("michi-micro-server") or services.get("michi-server") or services.get("michi")
            if svc:
                image = svc.get("image")
                if image != expected_image:
                    errors.append(f"Image mismatch in {os.path.relpath(comp_path, ROOT_DIR)}: {image!r} != {expected_image!r}")

                # Verify canonical deployment platform
                env = svc.get("environment", [])
                actual_platform = None
                if isinstance(env, dict):
                    actual_platform = env.get("MICHI_DEPLOYMENT_PLATFORM")
                elif isinstance(env, list):
                    for item in env:
                        if isinstance(item, str) and item.startswith("MICHI_DEPLOYMENT_PLATFORM="):
                            actual_platform = item.split("=", 1)[1].strip()
                            break

                if actual_platform != expected_platform:
                    errors.append(
                        f"Deployment platform mismatch in {os.path.relpath(comp_path, ROOT_DIR)}: "
                        f"expected {expected_platform!r}, got {actual_platform!r}"
                    )

    if errors:
        print("\n❌ Version Consistency Errors:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        sys.exit(1)

    print("✅ All version and packaging contracts are consistent.")

if __name__ == "__main__":
    main()
