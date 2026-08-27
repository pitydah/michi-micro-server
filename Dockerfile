ARG RUST_VERSION=1.88.0
ARG DEBIAN_VERSION=bookworm-slim

# Builder stage
FROM rust:${RUST_VERSION} AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy all manifests
COPY Cargo.toml Cargo.lock* ./
COPY apps/michi-server/Cargo.toml ./apps/michi-server/Cargo.toml
COPY crates/michi-core/Cargo.toml ./crates/michi-core/Cargo.toml
COPY crates/michi-api/Cargo.toml ./crates/michi-api/Cargo.toml
COPY crates/michi-config/Cargo.toml ./crates/michi-config/Cargo.toml
COPY crates/michi-db/Cargo.toml ./crates/michi-db/Cargo.toml
COPY crates/michi-metadata/Cargo.toml ./crates/michi-metadata/Cargo.toml
COPY crates/michi-scanner/Cargo.toml ./crates/michi-scanner/Cargo.toml
COPY crates/michi-streaming/Cargo.toml ./crates/michi-streaming/Cargo.toml
COPY crates/michi-m3u/Cargo.toml ./crates/michi-m3u/Cargo.toml
COPY crates/michi-sync/Cargo.toml ./crates/michi-sync/Cargo.toml
COPY crates/michi-homeassistant/Cargo.toml ./crates/michi-homeassistant/Cargo.toml
COPY crates/michi-tui/Cargo.toml ./crates/michi-tui/Cargo.toml
COPY crates/michi-client/Cargo.toml ./crates/michi-client/Cargo.toml
COPY crates/michi-opensubsonic/Cargo.toml ./crates/michi-opensubsonic/Cargo.toml
COPY crates/michi-rooms/Cargo.toml ./crates/michi-rooms/Cargo.toml
COPY crates/michi-link/Cargo.toml ./crates/michi-link/Cargo.toml
COPY crates/michi-receivers/Cargo.toml ./crates/michi-receivers/Cargo.toml
COPY crates/michi-security/Cargo.toml ./crates/michi-security/Cargo.toml
COPY crates/michi-ingest/Cargo.toml ./crates/michi-ingest/Cargo.toml
COPY crates/michi-connect/Cargo.toml ./crates/michi-connect/Cargo.toml
COPY crates/michi-onboard/Cargo.toml ./crates/michi-onboard/Cargo.toml
COPY vendor/michi-link/crates/michi-identity/Cargo.toml ./vendor/michi-link/crates/michi-identity/Cargo.toml

# Dummy sources for dependency caching
RUN for dir in michi-core michi-api michi-config michi-db michi-metadata michi-scanner michi-streaming michi-m3u michi-sync michi-homeassistant michi-tui michi-client michi-opensubsonic michi-rooms michi-link michi-receivers michi-security michi-ingest michi-connect michi-onboard; do \
      mkdir -p crates/$dir/src && echo "pub fn placeholder() {}" > crates/$dir/src/lib.rs; \
    done && \
    mkdir -p vendor/michi-link/crates/michi-identity/src && \
      echo "pub fn placeholder() {}" > vendor/michi-link/crates/michi-identity/src/lib.rs && \
    mkdir -p apps/michi-server/src && echo "fn main() {}" > apps/michi-server/src/main.rs && \
    cargo build --release --package michi-server 2>&1 || { echo "dependency caching step completed (build may have warnings)"; }

# Copy real source and rebuild
COPY apps ./apps
COPY crates ./crates
COPY vendor ./vendor

RUN find apps crates vendor -type f \( \
      -name '*.rs' -o -name '*.html' -o -name '*.css' -o -name '*.js' -o \
      -name '*.json' -o -name '*.svg' -o -name '*.png' -o -name '*.webp' \
    \) -exec touch {} + && \
    cargo build --release --package michi-server && \
    strip target/release/michi-server

# Runtime stage
FROM debian:${DEBIAN_VERSION}

LABEL org.opencontainers.image.title="michi-micro-server" \
      org.opencontainers.image.description="Michi Micro Server - High performance, resource-efficient music server for edge appliances" \
      org.opencontainers.image.source="https://github.com/pitydah/michi-micro-server" \
      org.opencontainers.image.licenses="MIT"

RUN apt-get update && apt-get install -y --no-install-recommends \
    libsqlite3-0 \
    wget \
    ca-certificates \
    ffmpeg \
    gosu \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /config /cache /music && \
    groupadd -r michi && \
    useradd -r -g michi -d /config -s /sbin/nologin michi && \
    chown -R michi:michi /config /cache /music

COPY --from=builder /app/target/release/michi-server /usr/local/bin/michi-server
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 9090

VOLUME ["/config", "/cache", "/music"]

ENV MICHI_PORT=9090
ENV MICHI_MUSIC_PATH=/music
ENV MICHI_CONFIG_PATH=/config
ENV MICHI_CACHE_PATH=/cache
ENV MICHI_DATABASE=sqlite:///config/michi.db
ENV PUID=1000
ENV PGID=1000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget -qO- http://127.0.0.1:9090/health/live || exit 1

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]

