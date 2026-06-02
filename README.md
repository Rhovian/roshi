# Roshi

Roshi is a Solana-native vault protocol built in native Rust with Wincode
serialization. It provides the on-chain program shape for a generalized vault
system with strategist-managed CPI execution, trusted NAV reporting, share
accounting, access control, and queued withdrawals.

## Workspace

- `crates/interface`: reusable Roshi protocol/interface types.
- `crates/roshi`: on-chain Roshi program crate.
- `crates/client`: thin client helpers for building Roshi instructions.
- `crates/tests`: LiteSVM and Surfpool-oriented integration test harness.

## Current Status

The program includes the reusable Solana infrastructure:

- Native Solana program entrypoint and Wincode instruction dispatch.
- `initialize_program` with a `ProgramConfig` PDA and authority storage.
- Generic indexed CPI execution in `manage` and `manage_batch`.
- Vault-scoped RBAC, pause flags, and subaccount PDA signer authorities.
- Surfpool config/script and Justfile targets.
- LiteSVM tests for program initialization, authorized CPI execution, deposits,
  redemptions, and withdrawal settlement.

The Roshi protocol surface currently includes:

- State for `ProgramConfig`, `Vault`, `Asset`, `Action`, `Ops`, `Op`, and
  `WithdrawalTicket`.
- PDA helper seeds for program config, vaults, subaccounts, actions, and
  withdrawal tickets.
- Authorization hash helper for ops-based CPI patterns.
- Implemented instruction handlers for program/vault initialization, action
  authorization/revocation, supported asset config, deposits, redemptions,
  redeem cancellation, queued withdrawal settlement, trusted NAV reporting,
  pause/access flags, role rotation, program/vault authority transfer, vault
  config updates, and strategist CPI execution.
- Instruction handlers are grouped by domain under `admin`, `execution`, and
  `user` modules where that grouping carries its weight.

The main remaining protocol work is fee crystallization and operational
tooling:

- Finalize performance-fee mechanics around `performance_fee_bps`,
  `high_watermark`, and `fee_collector`.
- Expand operational tooling around NAV reports, strategist workflows, and
  deployment/runbooks.

## Design Docs

- [Design Principles](docs/design.md)
- [Accounting](docs/accounting.md)
- [Accounting Math](docs/math.md)
- [Vault Access](docs/access.md)
- [NAV Reporting](docs/nav_reporting.md)
- [Oracles](docs/oracles.md)
- [Execution](docs/execution.md)
- [Subaccounts](docs/subaccounts.md)
- [RBAC](docs/rbac.md)
- [Withdrawals](docs/withdrawals.md)

## Dependencies

The dependency stack stays on the compatible Solana 3.x test/program ecosystem:

- Program-facing crates use current Solana 3.x minors where compatible.
- `solana-pubkey` is on 4.x.
- `wincode` is on 0.5.x.
- `litesvm` is on 0.12.x.
- `Cargo.lock` is checked in for reproducible program/test builds.

## Development

```bash
just build
just check
just surfpool-test
just test-sbf
```

`just build` produces `target/deploy/roshi.so`. The LiteSVM tests use that SBF
artifact when present, and `just surfpool-test` starts a Surfpool mainnet fork
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
