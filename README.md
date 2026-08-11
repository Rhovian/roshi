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
invariant-fuzzing harness that uses LibAFL and LiteSVM. It sends `roshi-client`
instructions to the real program. sBPF edge coverage on the LiteSVM execution
guides the fuzzer, so the program needs no instrumentation. After each mutated
action sequence, `invariant_core` evaluates the invariants below.

**Post-sequence invariants**

- Base tokens remain conserved. The program mints and burns shares only.
- Each registered non-base asset stays conserved in its own units. Its atoms
  never enter the base sum; deposits credit `total_assets` in priced base terms.
- Neither `performance_fee_bps` nor `withdrawal_buffer_bps` exceeds `MAX_BPS`.
- `high_watermark` never decreases, so the same gains cannot incur performance
  fees twice.
- `requested_withdrawal_shares` matches shares on unstruck live tickets.
  `pending_withdrawal_assets` matches assets owed by all live tickets.

After every accepted `report_nav`, net `total_assets` plus `fees_payable` plus
`pending_withdrawal_assets` equals gross NAV. A mismatch exposes an error in the
fee or liability arithmetic.

**What the harness exercises**

- The core loop runs deposits, redeems, NAV reports, and withdrawal settlement.
- `manage`, `manage_batch`, `swap`, and `atomic_redeem` run arbitrary CPIs that
  the program authorizes in advance.
  - `authorize_action` creates the authorization. `validate_authorized_cpi`
    validates it, and sub-account `invoke_signed` executes the CPI.
  - `manage` and `manage_batch` scan every writable custody account for the
    sub-account before a CPI and re-check it after. No route can leave a sibling
    custody with a delegate or close authority for a later drain.
  - `swap` must stay within its realized input and output bounds.
  - `atomic_redeem` must stay within the share entitlement. Its unwind must land
    in custody before the program burns the shares.
- A tampered `manage` cannot move custody funds to an unpinned destination.
- After `revoke_action`, a `manage` call for the same action moves nothing.
- The vault starts in private mode over a real access merkle tree. Members
  submit proofs with their deposits, so the core loop passes through the ACL by
  default. `set_vault_access` toggles the mode; private mode rejects a
  non-whitelisted outsider.
- Actions for non-base deposits use a mock Pyth feed. At a clean first deposit,
  they accept a fresh price. They reject a stale price, an over-wide confidence
  interval, or a disabled asset without moving tokens.
- Other actions deposit and swap a registered bare Token-2022 asset.
- A separate action proves that `initialize_asset` rejects a transfer-fee
  Token-2022 mint without creating its Asset PDA.
- Rotation actions prove that each previous signer loses its former instruction.
  They cover the program authority, vault admin, strategist, swap authority,
  NAV authority, and withdrawal authority.
- Configuration actions call `update_vault_config` or `set_pause_flags`. Through
  `update_vault_config`, one action replaces the full profile for economic
  controls.
- Separate sub-accounts hold deposit and withdrawal custody. `report_nav` counts
  both, but `process_withdrawals` can pay only from withdrawal custody.
- Fee actions preserve exact accounting. `collect_fees` and `write_down_fees`
  reject amounts above `fees_payable` without changing the vault or token
  balances.
- A deposit followed immediately by `atomic_redeem` at flat NAV never returns
  more base than the user deposited.

A minimized seed corpus is committed in `fuzz/corpus`. The `fuzz`,
`fuzz-stateful`, and `fuzz-cov` recipes pass it with `--corpus-in`.
The `vendor/crucible` git submodule records a specific revision of the engine
fork. This fork pins `litesvm` 0.12 and `solana-pubkey` 4.x. The harness uses
`solana-instruction` 3.4 for `Instruction` and `solana-pubkey` 4.2 for `Pubkey`.
Both versions satisfy `roshi-client`'s 3.3 and 4.1 constraints, so the types
unify.

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

When a crash is worth keeping, minimize the input and fix the bug. Then commit
the minimized input under `fuzz/regressions/invariant_core/`. After the fix,
`just fuzz-regressions` must not reproduce the failure.

## Design Docs

- [Design Principles](docs/design.md)
- [Accounting](docs/accounting.md)
- [Accounting Math](docs/math.md)
- [Controls](docs/controls.md)
- [Oracles](docs/oracles.md)
- [Execution](docs/execution.md)

## License

Apache-2.0. See [LICENSE](LICENSE).
