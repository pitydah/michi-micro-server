.PHONY: fmt fmt-check check test clippy build run clean docker docker-up docker-dev-up docker-down ci install deb audit coverage watch

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

check:
	cargo check --workspace

test:
	cargo test --workspace

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

build:
	cargo build --release -p michi-server

run:
	cargo run -p michi-server

clean:
	cargo clean

docker:
	docker build -t michi-micro-server .

docker-up:
	docker compose up -d --build

docker-dev-up:
	docker compose -f docker-compose.dev.yml up -d --build

docker-down:
	docker compose down

ci: fmt-check check test clippy

install: build
	cp target/release/michi-server /usr/local/bin/michi-server

deb:
	cargo deb

audit:
	cargo audit

coverage:
	cargo tarpaulin --workspace --out html

watch:
	cargo watch -x check -x clippy -x test
