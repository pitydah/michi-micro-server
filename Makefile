.PHONY: fmt fmt-check check test clippy build run clean docker docker-up ci install deb

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

ci: fmt-check check test clippy docker

install: build
	cp target/release/michi-server /usr/local/bin/michi-server

deb:
	cargo deb
