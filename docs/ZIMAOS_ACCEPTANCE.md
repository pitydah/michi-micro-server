# ZimaOS Physical Acceptance Protocol

This document defines the strict physical acceptance checklist for verifying Michi Micro Server releases on physical ZimaOS and CasaOS edge hardware (e.g. Zimaboard 832, Zimablade 7700, Zimacube).

---

## 1. Scope & Acceptance Criteria

A release is certified for ZimaOS only if:
1. The official store metadata (`dist/store.json`, `dist/index.json`) is reachable from raw GitHub without authentication.
2. An installation via either Custom Store or Compose succeeds cleanly without manual container troubleshooting.
3. The Web UI renders fresh assets without stale Service Worker or browser cache locks.
4. The server truthful identity (`/api/v1/settings`) correctly reports `deployment_platform: "zimaos"`.
5. Configuration, database, and auth sessions persist across container restarts and upgrades.

---

## 2. Acceptance Checklist

### Step 1: Remote Store Distribution Verification
Run the verification script from any machine or the appliance:
```bash
python3 scripts/verify_zimaos_install.py \
  --remote-url "https://raw.githubusercontent.com/pitydah/michi-micro-server/main"
```
- [ ] `dist/store.json` responds HTTP 200 OK.
- [ ] `dist/apps/io.michi.micro-server/docker-compose.yml` responds HTTP 200 OK.
- [ ] `store_id` equals `io.michi.store`.
- [ ] Content hashes and icons match.

---

### Step 2: Appliance Installation (Physical ZimaOS)

1. Navigate to **App Store** > **Settings (top right)** > **Add Store**.
2. Enter:
   ```text
   https://raw.githubusercontent.com/pitydah/michi-micro-server/main
   ```
3. Confirm **Michi Micro Server** appears in the catalog with the correct icon and tagline.
4. Click **Install**. Set an administrator password when prompted.
5. Alternatively, deploy using **Custom Install** with `zimaos-store/Apps/MichiMicroServer/docker-compose.yml`.

- [ ] Container downloads and starts without exit or restart loops.
- [ ] Container is running on port 9090 (`network_mode: host`).

---

### Step 3: Runtime Identity & Diagnostics

SSH into the appliance or run via terminal:
```bash
# 1. Health check
curl -fsS http://127.0.0.1:9090/health/live
# Expected: "OK"

# 2. Server identity check
curl -fsS http://127.0.0.1:9090/api/v1/settings | jq '{version, deployment_platform, commit}'
```

- [ ] `/health/live` returns HTTP 200 `OK`.
- [ ] `deployment_platform` equals `"zimaos"`.
- [ ] `version` matches target release (e.g. `1.0.0-rc.2`).
- [ ] `commit` is present and matches the release commit hash.

---

### Step 4: Web UI Freshness & PWA Service Worker Verification

1. Open `http://<APPLIANCE_IP>:9090` in a browser.
2. Open Developer Tools (F12) > Console & Application tabs.
3. Inspect network headers for `GET /`:
   - `Cache-Control: no-cache, no-store, must-revalidate`
4. Inspect network headers for `GET /sw.js`:
   - `Cache-Control: no-cache, no-store, must-revalidate`
5. Inspect Application > Cache Storage:
   - Cache name should be `michi-<version>-<hash>`.
   - Previous stale caches (e.g. `michi-v11`) should be automatically deleted.
6. Navigate to **Settings > Overview**:
   - Verify that **Version**, **Commit**, and **Platform** ("zimaos") are displayed truthfully.

- [ ] DOM displays fresh release UI (Hero cat, modern settings layout, update status).
- [ ] Settings Overview shows correct Version and Platform.
- [ ] No JavaScript console errors.

---

### Step 5: Upgrade Path & Data Persistence Test

1. Create a playlist or scan a music directory.
2. Sign in and verify authentication session works.
3. Stop and recreate the container (simulating an update):
   ```bash
   docker stop michi-micro-server
   docker rm michi-micro-server
   # Re-launch with the same volume mappings
   ```
4. Access `http://<APPLIANCE_IP>:9090`:
   - Authentication session remains valid (no unexpected logout).
   - Scanned tracks and playlists remain intact in `/config/michi.db`.

- [ ] All database state preserved across container recreation.
- [ ] Passwords and auth tokens remain valid.
