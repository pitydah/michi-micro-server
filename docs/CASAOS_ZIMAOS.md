# CasaOS / ZimaOS Compatibility

Michi Micro Server is designed to run as a Docker container, making it compatible with CasaOS and ZimaOS app stores.

## CasaOS Installation (Future)

CasaOS apps are defined using `docker-compose.yml` files. Michi Micro Server provides a CasaOS-compatible compose file at `casaos/docker-compose.casaos.yml`.

### Requirements for CasaOS App Store submission:

1. A `docker-compose.yml` with the required CasaOS variables:
   - `${CONFIG_PATH}`
   - `${CACHE_PATH}`
   - `${MUSIC_PATH}`
   - `${TZ}`

2. Application icon (512x512 PNG)
3. Application description and screenshots

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

## ZimaOS Compatibility

ZimaOS is based on CasaOS, so the same Docker deployment method works. ZimaOS features a user-friendly app store and file manager for managing Docker containers.

## Multi-Architecture Support

Michi Micro Server is compiled for:
- `linux/amd64` — Intel/AMD 64-bit
- `linux/arm64` — ARM 64-bit (Raspberry Pi 3/4/5, Rockchip, Apple Silicon)

Future:
- `linux/arm/v7` — ARM 32-bit (Raspberry Pi 2/3)
