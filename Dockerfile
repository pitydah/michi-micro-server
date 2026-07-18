ARG RUST_VERSION=1.88
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
COPY crates/michi-identity/Cargo.toml ./crates/michi-identity/Cargo.toml
COPY crates/michi-connect/Cargo.toml ./crates/michi-connect/Cargo.toml
COPY crates/michi-onboard/Cargo.toml ./crates/michi-onboard/Cargo.toml

# Dummy sources for dependency caching
RUN for dir in michi-core michi-api michi-config michi-db michi-metadata michi-scanner michi-streaming michi-m3u michi-sync michi-homeassistant michi-tui michi-client michi-opensubsonic michi-rooms michi-link michi-receivers michi-security michi-ingest michi-identity michi-connect michi-onboard; do \
      mkdir -p crates/$dir/src && echo "pub fn placeholder() {}" > crates/$dir/src/lib.rs; \
    done && \
    mkdir -p apps/michi-server/src && echo "fn main() {}" > apps/michi-server/src/main.rs && \
    cargo build --release --package michi-server 2>&1 || echo "dependency caching step completed"

# Copy real source and rebuild
COPY apps ./apps
COPY crates ./crates

RUN cargo build --release --package michi-server && \
    strip target/release/michi-server

# Runtime stage
FROM debian:${DEBIAN_VERSION}

RUN apt-get update && apt-get install -y --no-install-recommends \
    libsqlite3-0 \
    wget \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /config /cache /music && \
    groupadd -r michi && \
    useradd -r -g michi -d /config -s /sbin/nologin michi && \
    chown -R michi:michi /config /cache /music

COPY --from=builder /app/target/release/michi-server /usr/local/bin/michi-server

EXPOSE 8096

VOLUME ["/config", "/cache", "/music"]

ENV MICHI_PORT=8096
ENV MICHI_MUSIC_PATH=/music
ENV MICHI_CONFIG_PATH=/config
ENV MICHI_CACHE_PATH=/cache
ENV MICHI_DATABASE=sqlite:///config/michi.db

USER michi

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget -qO- http://127.0.0.1:8096/api/status || exit 1

ENTRYPOINT ["michi-server"]
