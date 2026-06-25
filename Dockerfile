ARG RUST_VERSION=1.77
ARG DEBIAN_VERSION=bookworm-slim

FROM rust:${RUST_VERSION} AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY apps ./apps
COPY crates ./crates

RUN cargo build --release --package michi-server && \
    strip target/release/michi-server

FROM debian:${DEBIAN_VERSION}

RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    ca-certificates \
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
ENV MICHI_DATABASE=sqlite:///config/michi.db?mode=rwc

ENTRYPOINT ["michi-server"]
