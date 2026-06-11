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
- A coverage-guided invariant fuzzer (crucible: LibAFL + LiteSVM) for the core
  accounting loop.

## Workspace

- `crates/interface`: reusable protocol types, instruction args, and math.
- `crates/roshi`: on-chain Solana program.
- `crates/client`: instruction-building helpers.
- `crates/tests`: LiteSVM integration test harness.
- `fuzz`: crucible invariant-fuzzing harness — a standalone workspace, not a
  member (see [Fuzzing](#fuzzing)).
- `vendor/crucible`: the fuzzer engine, vendored as a git submodule.

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

Generate the Codama IDL:

```bash
cargo run -p roshi-interface --example generate_codama_idl
```

The generator writes `target/idl/roshi.codama.json` by default. Pass a path as
the final argument to choose a different output file.

## Fuzzing

`fuzz/` is a [crucible](https://github.com/asymmetric-research/crucible)
invariant-fuzzing harness (LibAFL + LiteSVM, sBPF edge-coverage guided). It drives
the real program through `roshi-client` instructions and, after every mutated
action sequence, checks accounting invariants — base-token conservation (the
program mints/burns only shares, never base), withdrawal-queue accounting
(`requested_withdrawal_shares` and `pending_withdrawal_assets` reconcile against
the live tickets), high-watermark monotonicity (never regresses, so performance
fees can't be double-charged), and NAV-report conservation (right after a report,
net `total_assets` + accrued fees + pending withdrawals equals gross NAV, pinning
the fee/liability arithmetic). Alongside the core deposit/redeem/NAV
loop it exercises the arbitrary-CPI surface: pre-authorized `manage`,
`manage_batch`, `swap`, and `atomic_redeem` CPIs drive the action-authorization
machinery (`authorize_action`, `validate_authorized_cpi`, sub-account
`invoke_signed`, the custody clean-check, swap input/output bounds, and the
atomic-redeem entitlement/unwind-into-custody checks with share burn). Two
negative invariants pin authorization: a tampered `manage` asserts an unpinned
destination can never move custody funds, and `revoke_action` is exercised by
revoking an action and asserting a `manage` against it then moves nothing. The
vault runs **private** over a real access merkle tree: members deposit with
their proofs, `set_vault_access` toggles the access mode, and a non-whitelisted
outsider asserts a private vault never admits a deposit. The harness also covers
Pyth-priced non-base assets, Token-2022 registered assets, extended Token-2022
mint rejection, split custody, role rotation, config updates, fee correctness,
and flat-NAV no-overpay. A minimized seed corpus is committed in `fuzz/corpus`
and loaded by default. The engine is a fork pinned as the `vendor/crucible`
submodule (litesvm 0.12 / solana 4.x, so its instruction types match the
program's).

One-time setup:

```bash
git submodule update --init vendor/crucible
cargo install --path vendor/crucible/crates/crucible-fuzz-cli
```

Run (each recipe rebuilds `roshi.so` first):

```bash
just fuzz             # stateless: full mutated sequence per iteration
just fuzz-stateful    # stateful: single action over a live state pool (faster)
just fuzz-cov         # LCOV + HTML coverage report (needs genhtml)
```

Crash triage and regression replay:

```bash
just fuzz-crashes                         # list recorded crashes
just fuzz-show <crash-file-or-path>       # inspect one recorded crash
just fuzz-replay <input-path>             # replay a raw crash or regression input
just fuzz-tmin <crash-filename>           # minimize one crash in place
just fuzz-tmin-all                        # minimize all recorded crashes
just fuzz-regressions                     # replay committed regression inputs
```

When a crash is worth keeping, minimize it, fix the bug, then commit the
minimized input under `fuzz/regressions/invariant_core/`. A fixed regression
should no longer reproduce when `just fuzz-regressions` replays it.

## Design Docs

- [Design Principles](docs/design.md)
- [Accounting](docs/accounting.md)
- [Accounting Math](docs/math.md)
- [Controls](docs/controls.md)
- [Oracles](docs/oracles.md)
- [Execution](docs/execution.md)

## License

Apache-2.0. See [LICENSE](LICENSE).
