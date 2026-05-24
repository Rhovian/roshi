.PHONY: build test surfpool-test

build:
	cargo build-sbf --manifest-path crates/roshi/Cargo.toml

test:
	cargo test

surfpool-test:
	./scripts/surfpool-test.sh
