# Accounting

Roshi denominates vault accounting in the base deposit asset. The accounting
model is share based: users own shares, and shares represent a pro rata claim on
the vault's net asset value.

## State

The vault stores:

```rust
total_assets: u64,
last_report_hash: [u8; 32],
total_shares: u64,
high_watermark: u64,
performance_fee_bps: u16,
withdrawal_buffer_bps: u16,
max_change_bps: u16,
min_update_interval: i64,
last_update_ts: i64,
```

`total_assets` is the vault's current economic NAV in base atoms.

`last_report_hash` is the commitment to the private NAV report bundle behind
the last accepted NAV update.

`total_shares` is the total supply of vault shares tracked by the vault.

`high_watermark` is the highest fee-adjusted share price previously observed.
It is used for performance fee accounting.

`withdrawal_buffer_bps` is the target percentage of total assets to keep idle in
withdrawal custody for immediate withdrawals.

`max_change_bps` and `min_update_interval` bound NAV updates.

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

`total_assets` is the last accepted NAV report, denominated in base atoms:

```rust
total_assets = reported_total_nav
```

The program does not try to recompute NAV from all vault positions. That would
either be infeasible on-chain or require disclosing proprietary strategy inputs.
Instead, a configured `nav_authority` reports total portfolio NAV in base atoms
and provides a hash commitment to the private report bundle.

Token account balances remain important, but for settlement liquidity rather
than NAV truth:

- immediate redemptions can only pay from actual base custody liquidity,
- queued withdrawal processing can only settle when the relevant custody account
  can pay,
- NAV can include positions that are not directly observable in the instruction.

See [NAV Reporting](./nav_reporting.md) for the trust boundary and report
commitment model.

## NAV Update Flow

NAV authority calls:

```rust
UpdateTotalAssets {
    total_assets,
    report_hash,
}
```

The program should:

- verify the caller is the vault `nav_authority`,
- enforce `min_update_interval`,
- enforce `max_change_bps`,
- store `total_assets`, `last_report_hash`, and `last_update_ts`.

The update should fail if arithmetic overflows or if the NAV change exceeds the
configured guardrail.

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
share_price = total_assets / total_shares
```

Handlers should not compute this as floating point. They should use checked
integer multiplication and division through the common math helpers.

When `total_shares == 0`, the first depositor initializes the share base from
the deposit's base atoms and the vault base mint decimals:

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
shares_to_mint = floor(base_atoms * total_shares / total_assets)
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
- increase `total_shares` by `shares_to_mint`,
- enforce `min_shares_out`.

Deposits should not change share price except for integer rounding. Deposits
that round to zero shares should fail.

## Redeems

Redeems burn shares at the current share price.

```text
assets_out = floor(shares * total_assets / total_shares)
```

The redeem flow should:

- reject new redeems while withdrawals are paused,
- not require private-vault allowlist membership,
- enforce `min_assets_out`,
- burn or otherwise account the user's shares,
- reduce `total_shares` by `shares`,
- reduce `total_assets` by `assets_out`,
- either pay immediately from liquidity owned by `vault.withdraw_sub_account` or
  create a withdrawal ticket.

Shares are burned before creating a queued withdrawal ticket. That prevents a
user from both keeping shares and being owed queued assets.

## Withdrawal Buffer

`withdrawal_buffer_bps` is a target, not a hard accounting bucket.

The idle target is:

```text
target_idle_assets = ceil(total_assets * withdrawal_buffer_bps / 10_000)
```

Strategists should manage deployed positions so the withdraw subaccount can
serve normal withdrawals. The vault does not store a separate reserved-assets
counter; custody token account balances are the source of truth for immediate
payment capacity.

## Guardrails

`min_update_interval` prevents rapid repeated NAV updates.

`max_change_bps` caps the magnitude of a single NAV update. A typical check is:

```text
delta = abs(new_total_assets - old_total_assets)
max_delta = floor(old_total_assets * max_change_bps / 10_000)
delta <= max_delta
```

The first NAV update, zero-asset edge cases, and admin recovery paths should be
handled explicitly in implementation.

## Fees

The vault stores:

```rust
performance_fee_bps: u16,
fee_collector: Pubkey,
high_watermark: u64,
```

Performance fees are intended to apply only when share price exceeds the high
watermark. The current scaffold has not finalized when fees are crystallized.

Open design choices:

- collect fees during NAV update,
- collect fees during deposit/redeem,
- collect fees through an explicit crank,
- mint fee shares to the collector,
- transfer assets to the collector.

Until this is finalized, deposit and redeem accounting should be designed so fee
crystallization can be inserted without changing user-facing share semantics.

## Invariants

- `total_assets` equals the last accepted NAV report after a successful NAV
  update.
- `last_report_hash` commits to the private report bundle for the last accepted
  NAV update.
- `total_shares` changes only when shares are minted or burned.
- Share accounting uses fixed 9-decimal share atoms.
- Deposits increase both assets and shares proportionally after normalization to
  base atoms.
- Redeems decrease both assets and shares proportionally.
- NAV updates must respect `min_update_interval` and `max_change_bps`.
- Withdrawal tickets represent assets already removed from share accounting.
- Custody token account balances are the payment source of truth for immediate
  withdrawals and queued withdrawal settlement.

## Non-Goals

- The base asset does not have an `Asset` PDA.
- The program does not consume USD-denominated oracle semantics on-chain.
- The program does not compose, invert, or route oracle values on-chain.
- The program does not recompute full portfolio NAV from every custody,
  strategy, or venue account.
- Redemptions are not multi-asset in the current design.
