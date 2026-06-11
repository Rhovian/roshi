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

# List recorded crashes for a harness.
fuzz-crashes test='invariant_core':
    crucible show roshi -C fuzz --crashes-dir fuzz/crashes/{{test}}

# Inspect one recorded crash. Pass the crash filename or path.
fuzz-show crash test='invariant_core':
    crucible show roshi {{crash}} -C fuzz --crashes-dir fuzz/crashes/{{test}}

# Replay one input file. Use this for raw crashes or committed regressions.
fuzz-replay input test='invariant_core': build
    crucible run roshi {{test}} -C fuzz --release --replay {{input}}

# Minimize one recorded crash in place; pass the filename under fuzz/crashes/{{test}}.
fuzz-tmin crash test='invariant_core': build
    crucible tmin roshi {{test}} {{crash}} -C fuzz --release

# Minimize all recorded crashes for a harness in place.
fuzz-tmin-all test='invariant_core': build
    crucible tmin roshi {{test}} --all -C fuzz --release

# Replay committed regression inputs. A fixed regression should not reproduce.
fuzz-regressions test='invariant_core': build
    #!/usr/bin/env zsh
    set -e
    files=(fuzz/regressions/{{test}}/*(.N))
    if (( $#files == 0 )); then
        echo "no fuzz regressions for {{test}}"
        exit 0
    fi
    for file in $files; do
        crucible run roshi {{test}} -C fuzz --release --replay "$file"
    done
