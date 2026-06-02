set shell := ["zsh", "-cu"]

build:
    cargo build-sbf --manifest-path crates/roshi/Cargo.toml

test:
    cargo test

test-sbf: build
    cargo test -p roshi-tests

check:
    cargo fmt -- --check
    cargo check
    cargo check -p roshi --no-default-features
    cargo test -p roshi
    cargo build-sbf --manifest-path crates/roshi/Cargo.toml
    cargo test -p roshi-tests
