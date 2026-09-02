#!/usr/bin/env python3
"""
ZimaOS / CasaOS App Store Package Builder

Scans zimaos-store/Apps/*, extracts compose configurations and metadata,
and produces the canonical ZimaOS distribution layout under dist/:
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

    for app_name in sorted(os.listdir(APPS_SRC)):
        app_dir = os.path.join(APPS_SRC, app_name)
        if not os.path.isdir(app_dir):
            continue

        compose_file = os.path.join(app_dir, "docker-compose.yml")
        if not os.path.exists(compose_file):
            print(f"Skipping {app_name}: no docker-compose.yml found")
            continue

        with open(compose_file, "r", encoding="utf-8") as f:
            compose_data = yaml.safe_load(f)

        x_casaos = compose_data.get("x-casaos", {})
        app_id = x_casaos.get("id", app_name.lower())

        target_app_dir = os.path.join(APPS_DIST, app_id)
        target_assets_dir = os.path.join(target_app_dir, "assets")
        os.makedirs(target_assets_dir, exist_ok=True)

        # Copy compose file
        shutil.copy2(compose_file, os.path.join(target_app_dir, "docker-compose.yml"))

        # Copy assets
        for asset in ["icon.svg", "thumbnail.png", "icon.png"]:
            src_asset = os.path.join(app_dir, asset)
            if os.path.exists(src_asset):
                shutil.copy2(src_asset, os.path.join(target_assets_dir, asset))

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
        print(f"  ✓ Packaged {app_name} (ID: {app_id})")

    index_data = {
        "version": "3.1",
        "name": "Michi Official App Store",
        "apps": catalog
    }

    with open(os.path.join(DIST_DIR, "index.json"), "w", encoding="utf-8") as f:
        json.dump(index_data, f, indent=2, ensure_ascii=False)

    print(f"Successfully generated ZimaOS store distribution at {DIST_DIR} with {len(catalog)} app(s).")

if __name__ == "__main__":
    build_store()
