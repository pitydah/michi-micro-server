# Resource Budget

## Target Values

| Resource | Target | Notes |
|----------|--------|-------|
| Memory (idle) | < 50 MB | Target for Core profile |
| Threads | 8-12 | Tokio runtime + scanner + sync + receivers (Release hard max: <= 16) |
| SQLite pool | 4 (Eco), 8 (Balanced), 16 (Performance) | Configurable via profile |
| Scanner workers | 1 (Eco), 2 (Balanced), 4 (Performance) | |
| Max transcodes | 0 (Eco), 2 (Balanced), 4 (Performance) | 0 = no FFmpeg |
| Artwork cache | 128 MB (Eco), 256 MB (Balanced), 512 MB (Performance) | LRU |
| Transcode cache | 512 MB (Eco), 1 GB (Balanced), 2 GB (Performance) | LRU with TTL |
| Docker image size | < 100 MB (Core), < 200 MB (Full) | Core = no FFmpeg |
| Polling frequency | 60s (frontend) | With document.hidden check |
| Periodic tasks | hourly (cleanup), daily (integrity) | |

## Measured Values (Reference Qualification)

> **Note**: The authoritative measurements for each CI run are stored dynamically in the `resource-budget.json` release evidence artifacts. The table below documents the canonical reference qualification baseline.

<!-- BEGIN GENERATED RESOURCE MEASUREMENTS -->
| Resource | Release Limit | Measured | Environment |
|----------|---------------|----------|-------------|
| Idle RSS p95 | < 50.0 MB | 24.91 MB | Reference baseline Linux x86_64 (commit d3a48360) |
| Threads p95 | <= 16 | 13 | 1 main + 4 Tokio + 1 mDNS + 7 SQLx workers |
| FDs p95 | <= 128 | 14 | Linux procfs fd inspection |
| Startup | <= 5000 ms | 62.4 ms | Warm-up to /health/live HTTP 200 |
<!-- END GENERATED RESOURCE MEASUREMENTS -->

### Thread Taxonomy & Architecture (13 Active Threads)
Under the default `balanced` profile, the server operates 13 threads:
- **Main Thread (1)**: Signal handling, process lifecycle, startup orchestrator.
- **Tokio Worker Pool (4)**: Async multi-thread runtime (`worker_threads(4)`).
- **Service Discovery Daemon (1)**: Dedicated `mDNS_daemon` thread for zero-conf receiver discovery.
- **SQLite Worker Pool (7)**: Dedicated synchronous SQLite workers observed under qualification workload (sqlx max pool size: 8).
*Total*: 13 threads, well within the release hard budget of <= 16 threads.

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
