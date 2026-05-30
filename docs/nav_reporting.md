# NAV Reporting

Roshi treats NAV as an explicit trust boundary.

Some strategies cannot publish every input used to compute NAV without exposing
positions, counterparties, venue balances, or trading logic. The program should
not pretend to recompute that value trustlessly on-chain in v1.

## Model

The vault stores the last accepted total NAV report:

```rust
total_assets: u64,
last_report_hash: [u8; 32],
last_update_ts: i64,
max_change_bps: u16,
min_update_interval: i64,
```

`total_assets` is denominated in vault base atoms.

`last_report_hash` commits to the private report bundle used to produce the NAV.
The report bundle can contain whatever the vault team, auditor, or investor
process requires: position snapshots, venue statements, off-chain balances,
internal marks, or reconciliation output.

## Update Flow

The configured `nav_authority` calls:

```rust
UpdateTotalAssets {
    total_assets,
    report_hash,
}
```

The program should:

- verify the caller is `vault.nav_authority`,
- enforce `min_update_interval`,
- enforce `max_change_bps`,
- store `total_assets`,
- store `last_report_hash = report_hash`,
- store `last_update_ts`.

The report hash is a commitment, not public disclosure. It gives vault teams and
auditors a stable reference to the private report without revealing strategy
inputs on-chain.

## NAV Versus Liquidity

NAV and liquidity are separate concepts:

```text
NAV = nav_authority reported total assets in base atoms
Liquidity = actual token balances available for settlement
```

Token balances are still checked when the program needs to pay:

- immediate base-asset redemptions,
- queued withdrawal settlement,
- fee collection,
- strategy withdrawals.

Those balances are settlement capacity. They are not used to recompute total
NAV.

## Guardrails

`max_change_bps` limits the size of a single accepted NAV move.

`min_update_interval` limits update frequency.

Future implementations may add stricter report validation without changing
share accounting:

- signed report bundles,
- report schema versions,
- independent attestor signatures,
- multi-reporter quorum,
- challenge periods,
- selective proofs for positions that can be safely disclosed,
- private or zero-knowledge computation for sensitive strategy components.

## Invariants

- Only `nav_authority` can submit NAV reports.
- `total_assets` equals the last accepted NAV report.
- `last_report_hash` commits to the report bundle for `total_assets`.
- Deposits and redemptions update `total_assets` according to share accounting
  between NAV reports.
- Token account balances determine whether a payment can settle, not what NAV
  is.
