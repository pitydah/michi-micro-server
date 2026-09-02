#!/usr/bin/env python3
"""
ZimaOS / CasaOS App Store Package Builder & Validator

Scans zimaos-store/Apps/*, extracts compose configurations and metadata,
validates schema integrity, and produces the canonical ZimaOS distribution layout under dist/:
  dist/
  ├── index.json
  └── apps/
      └── <app-id>/
          ├── docker-compose.yml
          ├── meta.json
          └── assets/
              ├── icon.svg
              └── thumbnail.png
"""

import os
import sys
import json
import shutil
import yaml

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STORE_SRC = os.path.join(ROOT_DIR, "zimaos-store")
APPS_SRC = os.path.join(STORE_SRC, "Apps")
DIST_DIR = os.path.join(ROOT_DIR, "dist")
APPS_DIST = os.path.join(DIST_DIR, "apps")

def build_store():
    print(f"Building ZimaOS App Store packages from {STORE_SRC} -> {DIST_DIR}")
    
    if os.path.exists(DIST_DIR):
        shutil.rmtree(DIST_DIR)
    os.makedirs(APPS_DIST, exist_ok=True)

    catalog = []
    
    if not os.path.exists(APPS_SRC):
        print(f"ERROR: Apps directory {APPS_SRC} does not exist")
        sys.exit(1)

    app_dirs = [d for d in sorted(os.listdir(APPS_SRC)) if os.path.isdir(os.path.join(APPS_SRC, d))]
    if not app_dirs:
        print(f"ERROR: No applications found in {APPS_SRC}")
        sys.exit(1)

    for app_name in app_dirs:
        app_dir = os.path.join(APPS_SRC, app_name)
        compose_file = os.path.join(app_dir, "docker-compose.yml")
        if not os.path.exists(compose_file):
            print(f"ERROR: {app_name} missing required docker-compose.yml")
            sys.exit(1)

        with open(compose_file, "r", encoding="utf-8") as f:
            try:
                compose_data = yaml.safe_load(f)
            except Exception as e:
                print(f"ERROR: Failed to parse YAML in {compose_file}: {e}")
                sys.exit(1)

        x_casaos = compose_data.get("x-casaos", {})
        app_id = x_casaos.get("id")
        if not app_id:
            print(f"ERROR: {app_name} docker-compose.yml missing x-casaos.id")
            sys.exit(1)

        # Validate services and labels
        services = compose_data.get("services", {})
        if not services:
            print(f"ERROR: {app_name} docker-compose.yml contains no services")
            sys.exit(1)

        has_icon_label = any(
            isinstance(s.get("labels"), dict) and "icon" in s.get("labels", {})
            for s in services.values()
        )
        if not has_icon_label:
            print(f"ERROR: {app_name} service labels missing required 'icon' label")
            sys.exit(1)

        target_app_dir = os.path.join(APPS_DIST, app_id)
        target_assets_dir = os.path.join(target_app_dir, "assets")
        os.makedirs(target_assets_dir, exist_ok=True)

        # Copy compose file
        shutil.copy2(compose_file, os.path.join(target_app_dir, "docker-compose.yml"))

        # Copy & validate assets
        icon_svg = os.path.join(app_dir, "icon.svg")
        thumbnail_png = os.path.join(app_dir, "thumbnail.png")

        if not os.path.exists(icon_svg) or os.path.getsize(icon_svg) == 0:
            print(f"ERROR: {app_name} missing or empty icon.svg asset")
            sys.exit(1)
        shutil.copy2(icon_svg, os.path.join(target_assets_dir, "icon.svg"))

        if not os.path.exists(thumbnail_png) or os.path.getsize(thumbnail_png) == 0:
            print(f"ERROR: {app_name} missing or empty thumbnail.png asset")
            sys.exit(1)
        shutil.copy2(thumbnail_png, os.path.join(target_assets_dir, "thumbnail.png"))

        meta = {
            "id": app_id,
            "title": x_casaos.get("title", {"en_US": app_name}),
            "tagline": x_casaos.get("tagline", {}),
            "description": x_casaos.get("description", {}),
            "author": x_casaos.get("author", "pitydah"),
            "developer": x_casaos.get("developer", "pitydah"),
            "category": x_casaos.get("category", "Music"),
            "icon": f"apps/{app_id}/assets/icon.svg",
            "thumbnail": f"apps/{app_id}/assets/thumbnail.png",
            "port_map": x_casaos.get("port_map", "9090"),
            "scheme": x_casaos.get("scheme", "http"),
            "index": x_casaos.get("index", "/"),
            "version": x_casaos.get("version", "3.1"),
            "architectures": x_casaos.get("architectures", ["amd64", "arm64"]),
            "main_service": x_casaos.get("main", app_name)
        }

        with open(os.path.join(target_app_dir, "meta.json"), "w", encoding="utf-8") as f:
            json.dump(meta, f, indent=2, ensure_ascii=False)

        catalog.append(meta)
        print(f"  ✓ Packaged and validated {app_name} (ID: {app_id}, Version: {meta['version']})")

    index_data = {
        "version": "3.1",
        "name": "Michi Official App Store",
        "apps": catalog
    }

    with open(os.path.join(DIST_DIR, "index.json"), "w", encoding="utf-8") as f:
        json.dump(index_data, f, indent=2, ensure_ascii=False)

    print(f"Successfully generated and validated ZimaOS store distribution at {DIST_DIR} with {len(catalog)} app(s).")

if __name__ == "__main__":
    build_store()
