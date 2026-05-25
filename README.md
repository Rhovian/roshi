# Roshi

Roshi is a Solana-native vault protocol scaffold built in native Rust with
Wincode serialization. It provides the initial on-chain program shape for a
generalized vault system with operator-managed CPI execution, trusted NAV
reporting, share accounting, and queued withdrawals.

## Workspace

- `crates/common`: shared program constants and Pubkey helpers.
- `crates/roshi`: on-chain Roshi program crate.
- `tests`: LiteSVM and Surfpool-oriented integration test harness.

## Current Status

The scaffold includes the reusable Solana program infrastructure:

- Native Solana program entrypoint and Wincode instruction dispatch.
- `initialize_program` with a `ProgramConfig` PDA and authority storage.
- Generic indexed CPI execution in `manage` and `manage_batch`.
- Surfpool config/script and Makefile targets.
- LiteSVM tests for program initialization and authorized CPI execution.

It also includes the Roshi protocol surface:

- State scaffolding for `Vault`, `Action`, `Ops`, `Op`, and
  `WithdrawalTicket`.
- PDA helper seeds for program config, vaults, actions, and withdrawal tickets.
- Authorization hash helper for ops-based CPI patterns.
- Instruction variants and handler stubs for vault initialization, action
  authorization/revocation, NAV updates, deposits, redemptions, claims,
  withdrawal processing, and vault config updates.

Most Roshi-specific protocol instructions are intentionally still stubs. The
remaining work is implementation: account validation, PDA creation, Action
authorization enforcement in `manage`, share accounting, NAV guardrails, token
transfers, withdrawal queue processing, fee collection, and oracle support.

## Dependencies

The dependency stack stays on the compatible Solana 3.x test/program ecosystem:

- Program-facing crates use current Solana 3.x minors where compatible.
- `solana-pubkey` is on 4.x.
- `wincode` is on 0.5.x.
- `litesvm` is on 0.12.x.
- `Cargo.lock` is checked in for reproducible program/test builds.

## Development

```bash
make build
make test
make surfpool-test
```

`make build` produces `target/deploy/roshi.so`. The LiteSVM tests use that SBF
artifact when present, and `make surfpool-test` starts a Surfpool mainnet fork
before running ignored fork tests.

Useful direct checks:

```bash
cargo fmt -- --check
cargo check
cargo check -p roshi --no-default-features
cargo test -p roshi-tests
cargo build-sbf --manifest-path crates/roshi/Cargo.toml
```

`cargo build-sbf` currently succeeds, though the Solana build tool still emits
warnings about the dual `cdylib`/`rlib` crate types and undefined syscall names
during post-processing.
