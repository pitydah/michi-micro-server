.PHONY: build test docker run clean install deb

build:
	cargo build --release

test:
	cargo test

docker:
	docker build -t michi-micro-server .

run:
	cargo run --release

clean:
	cargo clean

install: build
	cp target/release/michi-server /usr/local/bin/michi-server

deb:
	cargo deb
