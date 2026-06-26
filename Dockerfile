ARG RUST_VERSION=1.83
ARG DEBIAN_VERSION=bookworm-slim

# Builder stage
FROM rust:${RUST_VERSION} AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock* ./
COPY apps/michi-server/Cargo.toml ./apps/michi-server/Cargo.toml
COPY crates/michi-core/Cargo.toml ./crates/michi-core/Cargo.toml
COPY crates/michi-api/Cargo.toml ./crates/michi-api/Cargo.toml
COPY crates/michi-config/Cargo.toml ./crates/michi-config/Cargo.toml
COPY crates/michi-db/Cargo.toml ./crates/michi-db/Cargo.toml
COPY crates/michi-metadata/Cargo.toml ./crates/michi-metadata/Cargo.toml
COPY crates/michi-scanner/Cargo.toml ./crates/michi-scanner/Cargo.toml
COPY crates/michi-streaming/Cargo.toml ./crates/michi-streaming/Cargo.toml
COPY crates/michi-homeassistant/Cargo.toml ./crates/michi-homeassistant/Cargo.toml
COPY crates/michi-sync/Cargo.toml ./crates/michi-sync/Cargo.toml
COPY crates/michi-multiroom/Cargo.toml ./crates/michi-multiroom/Cargo.toml

# Dummy main.rs to build dependencies (will be replaced)
RUN mkdir -p apps/michi-server/src && \
    echo "fn main() {}" > apps/michi-server/src/main.rs && \
    cargo build --release --package michi-server 2>/dev/null || true

# Copy real source and rebuild
COPY apps ./apps
COPY crates ./crates

RUN cargo build --release --package michi-server && \
    strip target/release/michi-server

# Runtime stage
FROM debian:${DEBIAN_VERSION}

RUN apt-get update && apt-get install -y --no-install-recommends \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /config /cache /music

COPY --from=builder /app/target/release/michi-server /usr/local/bin/michi-server

EXPOSE 8096

VOLUME ["/config", "/cache", "/music"]

ENV MICHI_PORT=8096
ENV MICHI_MUSIC_PATH=/music
ENV MICHI_CONFIG_PATH=/config
ENV MICHI_CACHE_PATH=/cache
ENV MICHI_DATABASE=sqlite:///config/michi.db

ENTRYPOINT ["michi-server"]
