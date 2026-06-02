# NAV Reporting

Roshi treats NAV as an explicit trust boundary.

Some strategies cannot publish every input used to compute NAV without exposing
positions, counterparties, venue balances, or trading logic. The program should
not pretend to recompute that value trustlessly on-chain in v1.

## Model

The vault stores the last accepted total NAV report:

```rust
total_assets: u64,
fees_payable: u64,
last_report_hash: [u8; 32],
last_update_ts: i64,
high_watermark: u64,
```

`total_assets` is fee-adjusted and denominated in vault base atoms.

`fees_payable` tracks base-asset fees accrued from NAV reporting but not yet
collected.

`last_report_hash` commits to the private report bundle used to produce the NAV.
The report bundle can contain whatever the vault team, auditor, or investor
process requires: position snapshots, venue statements, off-chain balances,
internal marks, or reconciliation output.

## Update Flow

The configured `nav_authority` calls:

```rust
ReportNav {
    total_assets,
    report_hash,
}
```

The program should:

- verify the caller is `vault.nav_authority`,
- reject an all-zero `report_hash`,
- read `share_mint.supply`,
- compute any performance fee against `high_watermark`,
- accrue the fee into `fees_payable`,
- store fee-adjusted `total_assets`,
- update `high_watermark`,
- store `last_report_hash = report_hash`,
- store `last_update_ts`.

The report hash is a commitment, not public disclosure. It gives vault teams and
auditors a stable reference to the private report without revealing strategy
inputs on-chain.

## NAV Versus Liquidity

NAV and liquidity are separate concepts:

```text
NAV = fee-adjusted total assets in base atoms
Liquidity = actual token balances available for settlement
```

Token balances are still checked when the program needs to pay:

- queued withdrawal settlement,
- fee collection,
- strategy withdrawals.

Those balances are settlement capacity. They are not used to recompute total
NAV.

## Fee Accrual

`ReportNav` treats `total_assets` in the instruction args as gross NAV. Gross
NAV must include the full portfolio value before subtracting Roshi-tracked
liabilities, including assets reserved or owed for pending withdrawals and
unpaid fees. Existing `fees_payable` and `pending_withdrawal_assets` are
subtracted from reported gross NAV before new fees are computed, so unpaid fees
and already-owed withdrawals do not leak back into active share accounting.

If gross share price exceeds `high_watermark` after those existing liabilities
are removed, the program accrues a base-asset performance fee, stores net NAV,
and records the fee in `fees_payable`.

Fee collection is separate from fee accrual. Admin-gated `CollectFees` transfers
base tokens from a supplied vault subaccount's custody account to the configured
fee collector token account and decrements `fees_payable`. Fees are
vault-scoped, not withdrawal-subaccount-scoped, and collection does not change
`total_assets` because NAV already excluded the fee.

## Future Trust Minimization

V1 intentionally trusts the configured NAV authority. A more trust-minimized
design should improve verification in layers without forcing every strategy
input to become public on-chain.

The first improvement is a signed report bundle. The on-chain instruction can
continue to store only `total_assets` and `report_hash`, while the private
bundle behind that hash contains the report schema version, position snapshots,
venue statements, liabilities, pricing inputs, reconciliation output, and
operator or auditor signatures. This does not make NAV trustless, but it makes
reports auditable and gives investors a stable artifact to verify.

A stronger model replaces the single NAV authority with a quorum of independent
attestors. The program can accept a NAV report only when enough approved
signers attest to the same `(vault, total_assets, report_hash, timestamp)`.
Attestors could include the manager, fund administrator, auditor, or an
independent pricing/reconciliation service.

Challenge periods are another possible layer. A submitted NAV report could
remain pending before it becomes the settlement baseline. Deposits and redeems
could either use the last finalized NAV or be restricted while a report is
pending. This adds operational complexity, but creates room for governance,
auditors, or investors to dispute bad reports before they affect settlement.

Publicly observable positions should be proven directly when possible. SPL
token custody balances, on-chain positions, LP positions, and oracle-priced
assets can be verified without relying on private disclosures. The trusted NAV
surface then shrinks to off-chain balances, private venues, liabilities, and
strategy-sensitive marks.

Longer term, report bundles can be Merkleized so individual sections can be
selectively disclosed:

```text
NAV report root
├── venue balances
├── on-chain positions
├── liabilities
├── pricing inputs
└── adjustments
```

Private or zero-knowledge computation may eventually prove that NAV was derived
from committed inputs without revealing the full strategy. That is a future
research path, not a requirement for the v1 trusted-authority model.

## Invariants

- Only `nav_authority` can submit NAV reports.
- `total_assets` equals the last accepted fee-adjusted NAV report.
- `fees_payable` tracks accrued fees already excluded from NAV.
- `last_report_hash` commits to the report bundle for `total_assets`.
- The all-zero report hash is reserved for "no accepted report yet".
- Deposits and redemptions update `total_assets` according to share accounting
  between NAV reports.
- Token account balances determine whether a payment can settle, not what NAV
  is.
