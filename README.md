# Roshi

Roshi is a Solana-native vault protocol for strategist-managed portfolios. It
combines share-based accounting, trusted NAV reporting, vault-scoped access
control, authorized strategy execution, queued withdrawals, and performance-fee
accounting.

## Disclaimer

Roshi is experimental software and has not been audited. It is provided as-is,
without warranties or liability. Do not use it with production funds unless you
have performed your own review, testing, and risk assessment.

## What Is Here

- On-chain program instructions for vault initialization, deposits, redemptions,
  queued withdrawal settlement, NAV reporting, fee collection, supported asset
  configuration, pause/access controls, role rotation, and authorized strategist
  CPI execution.
- Shared interface types and checked integer math used by the program, tests,
  and client helpers.
- Thin Rust client builders for Roshi instructions.
- LiteSVM integration tests covering the main protocol flows.

## Workspace

- `crates/interface`: reusable protocol types, instruction args, and math.
- `crates/roshi`: on-chain Solana program.
- `crates/client`: instruction-building helpers.
- `crates/tests`: LiteSVM integration test harness.

## Development

```bash
just build
just check
just test-sbf
```

Useful direct checks:

```bash
cargo fmt -- --check
cargo check
cargo check -p roshi --no-default-features
cargo test
cargo build-sbf --manifest-path crates/roshi/Cargo.toml
```

`just build` produces `target/deploy/roshi.so`. The integration tests use that
SBF artifact when present.

## Design Docs

- [Design Principles](docs/design.md)
- [Accounting](docs/accounting.md)
- [Accounting Math](docs/math.md)
- [Controls](docs/controls.md)
- [Oracles](docs/oracles.md)
- [Execution](docs/execution.md)

## License

Apache-2.0. See [LICENSE](LICENSE).
