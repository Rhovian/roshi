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

# Crucible invariant fuzzing (harness in fuzz/, engine vendored as the
# vendor/crucible submodule). One-time CLI install:
#   git submodule update --init vendor/crucible
#   cargo install --path vendor/crucible/crates/crucible-fuzz-cli
# `build` refreshes target/deploy/roshi.so, which the harness loads. `-C fuzz`
# points the CLI at the flattened harness dir.

# Stateless fuzz: full mutated sequence per iteration.
fuzz test='invariant_core' secs='60': build
    crucible run roshi {{test}} -C fuzz --release --corpus-in fuzz/corpus --timeout {{secs}}

# Stateful fuzz: one action per iteration over a live state pool (higher throughput).
fuzz-stateful test='invariant_core' secs='60' cores='8': build
    crucible run roshi {{test}} -C fuzz --release --stateful --cores {{cores}} --corpus-in fuzz/corpus --timeout {{secs}}

# Source-level coverage (LCOV + HTML; needs `genhtml` from lcov).
fuzz-cov test='invariant_core' secs='60': build
    crucible run roshi {{test}} -C fuzz --release --coverage --corpus-in fuzz/corpus --timeout {{secs}} --lcov-out fuzz/coverage/lcov.info
    genhtml fuzz/coverage/lcov.info -o fuzz/coverage/html
