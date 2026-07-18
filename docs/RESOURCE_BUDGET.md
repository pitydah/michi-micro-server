# Resource Budget

## Target Values

| Resource | Target | Notes |
|----------|--------|-------|
| Memory (idle) | < 50 MB | Target for Core profile |
| Threads | 8-12 | Tokio runtime + scanner + sync + receivers |
| SQLite pool | 4 (Eco), 8 (Balanced), 16 (Performance) | Configurable via profile |
| Scanner workers | 1 (Eco), 2 (Balanced), 4 (Performance) | |
| Max transcodes | 0 (Eco), 2 (Balanced), 4 (Performance) | 0 = no FFmpeg |
| Artwork cache | 128 MB (Eco), 256 MB (Balanced), 512 MB (Performance) | LRU |
| Transcode cache | 512 MB (Eco), 1 GB (Balanced), 2 GB (Performance) | LRU with TTL |
| Docker image size | < 100 MB (Core), < 200 MB (Full) | Core = no FFmpeg |
| Polling frequency | 60s (frontend) | With document.hidden check |
| Periodic tasks | hourly (cleanup), daily (integrity) | |

## Measured Values (to be filled)

| Resource | Target | Measured | Environment |
|----------|--------|----------|-------------|
| Memory (idle) | < 50 MB | -- | -- |
| Docker image size | < 100 MB (Core) | -- | -- |

## Resource Profiles

### Eco
- MICHI_RESOURCE_PROFILE=eco
- MICHI_SCAN_CONCURRENCY=1
- MICHI_MAX_TRANSCODES=0
- MICHI_DB_POOL_SIZE=4
- MICHI_ARTWORK_CACHE_MB=128
- MICHI_TRANSCODE_CACHE_MB=0

### Balanced (default)
- MICHI_RESOURCE_PROFILE=balanced
- MICHI_SCAN_CONCURRENCY=2
- MICHI_MAX_TRANSCODES=2
- MICHI_DB_POOL_SIZE=8
- MICHI_ARTWORK_CACHE_MB=256
- MICHI_TRANSCODE_CACHE_MB=512

### Performance
- MICHI_RESOURCE_PROFILE=performance
- MICHI_SCAN_CONCURRENCY=4
- MICHI_MAX_TRANSCODES=4
- MICHI_DB_POOL_SIZE=16
- MICHI_ARTWORK_CACHE_MB=512
- MICHI_TRANSCODE_CACHE_MB=2048

### Custom
Any combination of the above variables.
