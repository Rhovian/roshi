# Accounting

Roshi accounting is base-denominated and share-based. Users own shares, and
shares represent a pro rata claim on the vault's fee-adjusted net asset value.

## Vault State

The accounting-relevant vault fields are:

```rust
total_assets: u64,
pending_withdrawal_assets: u64,
fees_payable: u64,
last_report_hash: [u8; 32],
high_watermark: u64,
performance_fee_bps: u16,
fee_collector: Pubkey,
withdrawal_buffer_bps: u16,
last_update_ts: i64,
```

`total_assets` is active-share NAV in base atoms after subtracting Roshi-tracked
liabilities and newly accrued fees.

`pending_withdrawal_assets` is the vault-scoped base-asset amount owed across
open withdrawal tickets. It is not tied to any withdrawal subaccount.

`fees_payable` is the base-asset fee liability accrued during NAV reporting but
not yet transferred to the configured fee collector token account.

The total supply of shares is the SPL share mint's `supply` field. Roshi does
not mirror share supply in vault state.

## Supported Assets

The vault base mint is native to the vault and does not need an `Asset` PDA.
Additional deposit mints use vault-scoped `Asset` PDAs:

```text
[b"asset", vault, asset_mint]
```

Each `Asset` records the non-base mint, its custody token account,
base-denominated oracle configuration, decimal metadata, deposit limit, and
enabled state.

Deposit-time math normalizes each non-base amount into base atoms before
minting shares. The oracle must report the direct asset/base fixed-point value
consumed by Roshi. Roshi does not consume USD, inverse, or routed price
semantics on-chain.

See [Oracles](./oracles.md) for the oracle contract.

## NAV Reporting

`ReportNav` accepts gross total portfolio NAV in base atoms:

```rust
ReportNav {
    total_assets,
    report_hash,
}
```

The reported gross NAV must include the full portfolio value before subtracting
Roshi-tracked liabilities, including assets reserved or owed for open
withdrawal tickets and unpaid fees.

The program stores active-share NAV:

```text
fee_base_assets = reported_gross_nav - fees_payable - pending_withdrawal_assets
total_assets = fee_base_assets - newly_accrued_fees
```

`report_hash` commits to the private NAV report bundle. The bundle can contain
position snapshots, venue statements, off-chain balances, internal marks, or
reconciliation output. The all-zero report hash is reserved for "no accepted
report yet".

The NAV update flow:

- verify the caller is `vault.nav_authority`,
- reject an all-zero `report_hash`,
- read `share_mint.supply`,
- subtract existing `fees_payable` and `pending_withdrawal_assets`,
- accrue performance fees when share price exceeds `high_watermark`,
- store fee-adjusted `total_assets`,
- increase `fees_payable`,
- update `high_watermark`,
- store `last_report_hash` and `last_update_ts`.

The update fails if arithmetic overflows or if reported gross NAV is less than
existing fee and withdrawal liabilities.

NAV and liquidity are separate. Token account balances determine whether queued
withdrawals, fee collection, or strategy withdrawals can settle; balances do
not recompute total NAV.

## Share Price

Vault shares use fixed 9-decimal accounting:

```rust
SHARE_DECIMALS = 9
```

Share decimals do not inherit the vault base mint decimals.

Share price is derived from `total_assets` and the SPL share mint supply through
checked integer math. It is not stored directly.

When `share_mint.supply == 0`, the first depositor initializes the share base:

```text
initial_shares = floor(base_atoms * 10^SHARE_DECIMALS / 10^base_decimals)
```

For one whole base unit:

```text
USDC base, 6 decimals: 1_000_000 base atoms -> 1_000_000_000 share atoms
SOL base, 9 decimals: 1_000_000_000 base atoms -> 1_000_000_000 share atoms
```

See [Accounting Math](./math.md) for helper formulas and rounding behavior.

## Deposits

Deposits mint shares at the current share price after normalizing the deposit
amount into base atoms.

```text
shares_to_mint = floor(base_atoms * share_mint.supply / total_assets)
```

If the vault has no shares, deposits use the initial share formula above.

The deposit flow:

- reject deposits while deposits are paused,
- if the vault is private, verify the depositor's access proof,
- if `asset_mint == vault.base_mint`, transfer base assets into custody owned
  by `vault.deposit_sub_account`,
- otherwise load the `Asset` PDA, verify it is enabled, transfer the non-base
  assets into its configured custody token account, and compute `base_atoms`
  with the configured oracle,
- mint shares to the user,
- increase `total_assets` by `base_atoms`,
- enforce `min_shares_out`.

Deposits should not change share price except for integer rounding. Deposits
that round to zero shares fail.

## Redeems And Withdrawals

Redeems burn shares immediately and create queued withdrawal tickets.

```text
assets_owed = floor(shares * total_assets / share_mint.supply)
```

The redeem flow:

- reject new redeems while withdrawals are paused,
- not require private-vault allowlist membership,
- enforce `min_assets_out`,
- burn the user's shares,
- reduce `total_assets` by `assets_owed`,
- create a withdrawal ticket for later settlement.

Withdrawal ticket PDAs are bounded by vault, recipient token account, and ticket
index:

```text
[b"ticket", vault, recipient_token_account, ticket_index]
```

Each ticket records:

```rust
WithdrawalTicket {
    vault: Pubkey,
    owner: Pubkey,
    recipient_token_account: Pubkey,
    ticket_index: u8,
    shares_burned: u64,
    assets_owed: u64,
    bump: u8,
}
```

A recipient token account may have up to 256 open queued tickets per vault.
Reusing a slot requires the withdrawal authority to process and clear the
existing ticket first.

Tickets are vault-scoped user liabilities, not subaccount-scoped liabilities.
`vault.withdraw_sub_account` only selects the default custody source used when
the withdrawal authority pays open tickets.

`ProcessWithdrawals` settles supplied tickets:

- verify the caller is `vault.withdrawal_authority`,
- verify each ticket's vault, owner, recipient, PDA, bump, and nonzero
  `assets_owed`,
- verify the configured withdraw custody can pay,
- transfer owed base assets to each recorded recipient token account,
- close settled ticket accounts back to their owners,
- decrement `pending_withdrawal_assets`.

Processing is atomic. If any transfer cannot be paid, the instruction fails and
the tickets remain open.

## Withdrawal Buffer

`withdrawal_buffer_bps` is a target, not a hard accounting bucket.

```text
target_idle_assets = ceil(total_assets * withdrawal_buffer_bps / 10_000)
```

Strategists should manage deployed positions so withdrawal custody can settle
queued withdrawals. The vault does not store a separate reserved-assets counter;
custody token account balances are the source of truth for settlement capacity.

## Fees

Performance fees apply only when gross share price exceeds `high_watermark`.
Fees are denominated in base assets and never accrue as newly minted shares.

During `ReportNav`, existing `fees_payable` and `pending_withdrawal_assets` are
removed from the fee base:

```text
fee_base_assets = gross_total_assets - fees_payable - pending_withdrawal_assets
```

The program then computes newly accrued fees:

```text
gross_share_price = floor(fee_base_assets * 10^SHARE_DECIMALS / share_mint.supply)
high_watermark_assets = ceil(high_watermark * share_mint.supply / 10^SHARE_DECIMALS)
profit_assets = fee_base_assets - high_watermark_assets
new_fee = floor(profit_assets * performance_fee_bps / 10_000)
net_total_assets = fee_base_assets - new_fee
```

If `high_watermark == 0`, the report establishes the baseline and accrues no
fee. If gross share price does not exceed the high watermark, no fee accrues and
the high watermark is unchanged.

`CollectFees` settles an existing payable:

```rust
CollectFees {
    sub_account,
    amount,
}
```

The instruction is admin-gated. It transfers base tokens from the supplied
vault subaccount's custody account to the configured `fee_collector` token
account and decrements `fees_payable`. Collection does not change
`total_assets`; NAV already excluded the fee when it accrued.

## Future NAV Verification

V1 intentionally trusts the configured NAV authority. Future designs can reduce
trust by adding signed report bundles, a quorum of independent attestors,
challenge periods, public reconciliation leaves for verifiable assets, or
private/zero-knowledge computation. These are research paths, not requirements
for the v1 trusted-authority model.

## Invariants

- `total_assets` equals the last accepted fee-adjusted active-share NAV.
- `fees_payable` represents fees already excluded from `total_assets`.
- `pending_withdrawal_assets` represents assets already removed from active
  share accounting.
- `last_report_hash` commits to the private report bundle for the last accepted
  NAV update.
- Share mint supply changes only when shares are minted or burned.
- Deposits increase both assets and shares proportionally after normalization to
  base atoms.
- Redeems decrease both assets and shares proportionally.
- Withdrawal tickets are settled only by `ProcessWithdrawals`.
- Collecting fees does not change `total_assets`.
- Custody token account balances are the payment source of truth for
  withdrawals and fee collection.

## Non-Goals

- No base asset `Asset` PDA.
- No USD-denominated, inverse, composed, or routed oracle semantics on-chain.
- No on-chain recomputation of full portfolio NAV in v1.
- No multi-asset redemption path in the current design.
- No withdrawal solver market, discounts, maturity auctions, or deadline
  market.
