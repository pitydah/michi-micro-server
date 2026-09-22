# Michi Micro Server on CasaOS & ZimaOS

Michi Micro Server is packaged and optimized for edge appliances running CasaOS and ZimaOS. It runs as a low-overhead, native Docker service with host networking support for physical receiver discovery and local mDNS playback.

---

## 1. Storage & Volume Mapping

Michi follows standard CasaOS/ZimaOS directory conventions:

| Host Path (Default) | Container Path | Purpose | Access |
|---------------------|----------------|---------|--------|
| `/DATA/AppData/michi/config` | `/config` | SQLite database (`michi.db`), server secrets, auth sessions | Read / Write |
| `/DATA/AppData/michi/cache` | `/cache` | Audio transcode cache, artwork cache, temp sync storage | Read / Write |
| `/DATA/Media/Music` | `/music` | Music library root | Read Only |

---

## 2. Installation Methods

### Method A: Custom App Store (Recommended)

In ZimaOS / CasaOS App Store settings, add the official Michi Micro Server store source:

```text
https://raw.githubusercontent.com/pitydah/michi-micro-server/main
```

ZimaOS automatically fetches the catalog (`dist/store.json` and `dist/index.json`) and presents **Michi Micro Server** in your local App Store.

### Method B: Custom Install via Compose

In the CasaOS / ZimaOS dashboard, click **Install a customized app** (or **Custom Install**) and paste the canonical compose file from:
[`zimaos-store/Apps/MichiMicroServer/docker-compose.yml`](../zimaos-store/Apps/MichiMicroServer/docker-compose.yml)

### Method C: Command Line (Docker Compose)

```bash
# Create required directories
mkdir -p /DATA/AppData/michi/{config,cache}
mkdir -p /DATA/Media/Music

# Launch container with host networking
docker run -d \
  --name michi-micro-server \
  --network host \
  -v /DATA/AppData/michi/config:/config \
  -v /DATA/AppData/michi/cache:/cache \
  -v /DATA/Media/Music:/music:ro \
  -e TZ=UTC \
  -e PUID=1000 \
  -e PGID=1000 \
  -e MICHI_PORT=9090 \
  -e MICHI_DEPLOYMENT_PLATFORM=zimaos \
  -e MICHI_AUTH_USERNAME=admin \
  -e MICHI_AUTH_PASSWORD="YourStrongPasswordHere" \
  --restart unless-stopped \
  ghcr.io/pitydah/michi-micro-server:1.0.0-rc.2
```

---

## 3. Access & Networking

- **Default Web UI & API Port**: `http://<APPLIANCE_IP>:9090`
- **Tailscale**: `http://<TAILSCALE_DEVICE_NAME>:9090`
- **Host Network Mode**: Michi Micro Server uses `network_mode: host` to allow automatic local mDNS discovery of Michi Stream Hi-Fi receivers and Snapcast audio clients without complex Docker port forwards.

---

## 4. Multi-Architecture Support

Official container images are multi-architecture OCI indexes supporting:
- `linux/amd64` (Intel & AMD x86_64, Zimaboard, Zimablade)
- `linux/arm64` (Zimacube ARM, Raspberry Pi 4/5, Rockchip)

Canonical image repository:
```text
ghcr.io/pitydah/michi-micro-server:<version>
```

---

## 5. Diagnostics & Verification

To verify that your appliance is running the correct version and platform environment:

```bash
# Check container status and platform environment variable
docker inspect michi-micro-server --format '{{json .Config.Env}}' | jq .

# Verify live health endpoint
curl -fsS http://127.0.0.1:9090/health/live

# Verify server identity and platform reporting
curl -fsS http://127.0.0.1:9090/api/v1/settings | jq '{version, deployment_platform, commit}'
```

---

## 6. Frontend Cache & Service Worker Troubleshooting

Michi Micro Server uses a Progressive Web App (PWA) Service Worker with automatic cache-busting:
- `index.html` and `/sw.js` are served with `Cache-Control: no-cache, no-store, must-revalidate`.
- Static JavaScript and CSS bundles carry dynamic content hashes (`?v=<hash>`).
- On server upgrade, the Service Worker calls `skipWaiting()` and `clients.claim()` to purge previous caches immediately.

If you previously installed an older preview or RC and your browser continues to display an older UI:
1. In your browser, perform a hard refresh:
   - **Chrome / Firefox / Edge (Windows/Linux)**: `Ctrl + F5` or `Ctrl + Shift + R`
   - **Safari (Mac)**: `Cmd + Option + R`
2. Alternatively, open **Developer Tools** > **Application** > **Storage** > click **Clear site data**.
3. Verify in **Settings > Overview** that the reported **Version** and **Platform** match your running server.
