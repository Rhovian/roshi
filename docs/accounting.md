# Accounting

Roshi denominates vault accounting in the base deposit asset. The accounting
model is share based: users own shares, and shares represent a pro rata claim on
the vault's net asset value.

## State

The vault stores:

```rust
total_assets: u64,
fees_payable: u64,
last_report_hash: [u8; 32],
high_watermark: u64,
performance_fee_bps: u16,
fee_collector: Pubkey,
withdrawal_buffer_bps: u16,
last_update_ts: i64,
```

`total_assets` is the vault's current fee-adjusted economic NAV in base atoms.

`fees_payable` is the base-asset fee liability accrued during NAV reporting but
not yet transferred to the configured fee collector token account.

`last_report_hash` is the commitment to the private NAV report bundle behind
the last accepted NAV update.

The total supply of vault shares is the SPL share mint's `supply` field. Roshi
does not mirror that value in vault state.

`high_watermark` is the highest fee-adjusted share price previously observed.
It is used for performance fee accounting.

`fee_collector` is the configured base token account that receives collected
fees. `InitializeVault` and `UpdateVaultConfig` verify that this account is an
initialized SPL token account for the vault base mint.

`withdrawal_buffer_bps` is the target percentage of total assets to keep idle in
withdrawal custody for queued settlement.

## Supported Assets

The vault's base mint is native to the vault and does not need a supported
asset PDA. Additional deposit mints are represented by vault-scoped `Asset`
PDAs:

```text
[b"asset", vault, asset_mint]
```

Each `Asset` account records the non-base mint, its vault custody token account,
base-denominated oracle configuration, decimal metadata, deposit limit, and
enabled state.

Deposit-time math must normalize each non-base amount into base atoms before
minting shares. The oracle must report the relationship directly in vault base
atoms, such as base atoms per asset atom. Roshi does not consume USD, inverse,
or routed price semantics on-chain; the configured oracle value must already be
the direct `asset/base` fixed-point value consumed by Roshi.

Redemptions remain base-asset denominated. Multi-asset withdrawals are outside
the current scaffold.

See [Oracles](./oracles.md) for the base-denominated oracle contract.

## Total Assets

`total_assets` is the last accepted fee-adjusted NAV, denominated in base atoms:

```rust
fee_base_assets = reported_gross_nav - fees_payable - pending_withdrawal_assets
total_assets = fee_base_assets - newly_accrued_fees
```

The program does not try to recompute NAV from all vault positions. That would
either be infeasible on-chain or require disclosing proprietary strategy inputs.
Instead, a configured `nav_authority` reports gross total portfolio NAV in base
atoms and provides a hash commitment to the private report bundle. The program
then computes performance fees and stores net `total_assets`.

The reported gross NAV must include the vault's full portfolio value, including
assets reserved for open withdrawal tickets and assets that back already
accrued but uncollected fees. Those liabilities live in vault state and are
subtracted by the program before new fee math or active-share NAV storage.

Token account balances remain important, but for settlement liquidity rather
than NAV truth:

- queued withdrawal processing can only settle when the relevant custody account
  can pay,
- NAV can include positions that are not directly observable in the instruction.

See [NAV Reporting](./nav_reporting.md) for the trust boundary and report
commitment model.

## NAV Update Flow

NAV authority calls:

```rust
ReportNav {
    total_assets,
    report_hash,
}
```

The program should:

- verify the caller is the vault `nav_authority`,
- reject an all-zero `report_hash`,
- read `share_mint.supply`,
- accrue performance fees when gross share price exceeds `high_watermark`,
- store fee-adjusted `total_assets`,
- increase `fees_payable`,
- update `high_watermark`,
- store `last_report_hash` and `last_update_ts`.

The update should fail if arithmetic overflows or if reported gross NAV is less
than existing fee and withdrawal liabilities.

## Share Price

Vault shares use fixed 9-decimal accounting:

```rust
SHARE_DECIMALS = 9
```

Share decimals do not inherit the vault base mint decimals. A USDC-based vault
may have 6 base decimals and 9 share decimals. A SOL-based vault may have 9
base decimals and 9 share decimals.

Share price is the ratio between base atoms and share atoms:

```text
share_price = total_assets / share_mint.supply
```

Share price is not stored directly. Handlers should not compute it as floating
point; they should derive it from `total_assets` and the SPL share mint supply
through the checked integer math helpers.

When `share_mint.supply == 0`, the first depositor initializes the share base
from the deposit's base atoms and the vault base mint decimals:

```text
initial_shares = floor(base_atoms * 10^SHARE_DECIMALS / 10^base_decimals)
```

For one whole base unit, this gives:

```text
USDC base, 6 decimals: 1_000_000 base atoms -> 1_000_000_000 share atoms
SOL base, 9 decimals: 1_000_000_000 base atoms -> 1_000_000_000 share atoms
```

See [Accounting Math](./math.md) for the shared helper contract.

## Deposits

Deposits mint shares at the current share price after normalizing the deposit
amount into base atoms.

If the vault already has shares:

```text
shares_to_mint = floor(base_atoms * share_mint.supply / total_assets)
```

If the vault has no shares:

```text
shares_to_mint = floor(base_atoms * 10^SHARE_DECIMALS / 10^base_decimals)
```

The deposit flow should:

- reject deposits while deposits are paused,
- if the vault is private, verify the depositor's access proof against
  `vault.access_merkle_root`,
- reject access proofs longer than 32 sibling hashes,
- if `asset_mint == vault.base_mint`, transfer base assets from the user to the
  base custody account owned by `vault.deposit_sub_account`,
- otherwise load the `Asset` PDA, verify it is enabled, transfer the non-base
  assets into its configured custody token account, and use the configured
  oracle to compute `base_atoms`,
- mint or otherwise account shares to the user,
- increase `total_assets` by `base_atoms`,
- enforce `min_shares_out`.

Deposits should not change share price except for integer rounding. Deposits
that round to zero shares should fail.

## Redeems

Redeems burn shares at the current share price.

```text
assets_out = floor(shares * total_assets / share_mint.supply)
```

The redeem flow should:

- reject new redeems while withdrawals are paused,
- not require private-vault allowlist membership,
- enforce `min_assets_out`,
- burn or otherwise account the user's shares,
- reduce `total_assets` by `assets_out`,
- create a vault-scoped withdrawal ticket for later settlement to the recorded
  recipient.

Shares are burned before creating a queued withdrawal ticket. That prevents a
user from both keeping shares and being owed queued assets.

Withdrawal tickets are not scoped to the configured withdrawal subaccount. The
ticket records the vault, owner wallet, recipient token account, ticket index,
and assets owed. `vault.withdraw_sub_account` only selects the default custody
source used when the withdrawal authority later pays open tickets.

## Withdrawal Buffer

`withdrawal_buffer_bps` is a target, not a hard accounting bucket.

The idle target is:

```text
target_idle_assets = ceil(total_assets * withdrawal_buffer_bps / 10_000)
```

Strategists should manage deployed positions so the withdraw subaccount can
settle queued withdrawals. The vault does not store a separate reserved-assets
counter; custody token account balances are the source of truth for settlement
capacity.

## Fees

The vault stores:

```rust
performance_fee_bps: u16,
fee_collector: Pubkey,
fees_payable: u64,
high_watermark: u64,
```

Performance fees apply only when gross share price exceeds `high_watermark`.
Fees are denominated in base assets and never accrue as newly minted shares.
That avoids diluting existing shareholders.

During `ReportNav`, the NAV authority reports gross total assets. Gross means
the full portfolio value before subtracting Roshi-tracked liabilities, including
assets reserved or owed for pending withdrawals and unpaid fees. Existing
`fees_payable` and `pending_withdrawal_assets` are first removed from the fee
base so uncollected fees and already-owed withdrawals cannot leak back into
active-share NAV:

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

`ReportNav` stores `net_total_assets`, increments `fees_payable` by `new_fee`,
and updates `high_watermark` to the post-fee share price when that price exceeds
the old high watermark.

`CollectFees` settles an existing payable:

```rust
CollectFees {
    sub_account,
    amount,
}
```

The instruction is admin-gated by policy, transfers base tokens from
the supplied vault subaccount's base custody account to the configured
`fee_collector` token account, and decrements `fees_payable`. Fees are
vault-scoped, not withdrawal-subaccount-scoped. Collection does not change
`total_assets`; NAV already excluded the fee when it accrued.

## Invariants

- `total_assets` equals the last accepted fee-adjusted NAV after a successful
  NAV update.
- `fees_payable` represents fees already excluded from `total_assets`.
- `last_report_hash` commits to the private report bundle for the last accepted
  NAV update.
- Share mint supply changes only when shares are minted or burned.
- Share accounting uses fixed 9-decimal share atoms.
- Deposits increase both assets and shares proportionally after normalization to
  base atoms.
- Redeems decrease both assets and shares proportionally.
- Collecting fees does not change `total_assets`.
- Withdrawal tickets represent assets already removed from share accounting.
- Custody token account balances are the payment source of truth for queued
  withdrawal settlement.

## Non-Goals

- The base asset does not have an `Asset` PDA.
- The program does not consume USD-denominated oracle semantics on-chain.
- The program does not compose, invert, or route oracle values on-chain.
- The program does not recompute full portfolio NAV from every custody,
  strategy, or venue account.
- Redemptions are not multi-asset in the current design.
