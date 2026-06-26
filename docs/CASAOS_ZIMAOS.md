# CasaOS / ZimaOS Compatibility

Michi Micro Server is designed to run as a Docker container, making it compatible with CasaOS and ZimaOS app stores.

## CasaOS Metadata

CasaOS metadata lives in `casaos/`:

| File | Purpose |
|------|---------|
| `docker-compose.casaos.yml` | CasaOS-compatible compose file with `${}` variables |
| `data.yml` | App store listing metadata |

### Requirements for CasaOS App Store submission:

1. Application icon (512x512 PNG) — TODO: create icon
2. Application screenshots — TODO: capture UI screenshots
3. Published Docker image on ghcr.io — TODO: set up CI/CD

### CasaOS Variables

The compose file uses CasaOS template variables:

| Variable | Purpose |
|----------|---------|
| `${CONFIG_PATH}` | Persistent config storage |
| `${CACHE_PATH}` | Cache directory |
| `${MUSIC_PATH}` | Music library mount |
| `${TZ}` | Timezone |

### Manual Installation on CasaOS

```bash
# SSH into your CasaOS device
mkdir -p /DATA/AppData/michi-micro-server/{config,cache}
mkdir -p /DATA/Music

# Run with Docker
docker run -d \
  --name michi-micro-server \
  -p 8096:8096 \
  -v /DATA/AppData/michi-micro-server/config:/config \
  -v /DATA/AppData/michi-micro-server/cache:/cache \
  -v /DATA/Music:/music \
  -e TZ=America/Santiago \
  --restart unless-stopped \
  pitydah/michi-micro-server:latest
```

### Access

- **Local network**: `http://<CASAOS_IP>:8096`
- **Tailscale** (recommended): `http://<TAILSCALE_HOST>:8096`
- **Do not** expose port 8096 directly to the internet.

## ZimaOS Compatibility

ZimaOS is based on CasaOS, so the same Docker deployment method works.

## Multi-Architecture Support

Michi Micro Server is compiled for:
- `linux/amd64` — Intel/AMD 64-bit
- `linux/arm64` — ARM 64-bit (Raspberry Pi 3/4/5, Rockchip, Apple Silicon)

## TODO (Pre-Submission)

- [ ] Create 512x512 PNG application icon
- [ ] Capture Web UI screenshots
- [ ] Publish Docker image to ghcr.io/pitydah/michi-micro-server
- [ ] Add CI/CD workflow for multi-arch builds
- [ ] Test on actual CasaOS/ZimaOS device
