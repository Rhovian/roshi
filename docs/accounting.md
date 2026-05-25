# Accounting

Roshi denominates vault accounting in the base deposit asset. The accounting
model is share based: users own shares, and shares represent a pro rata claim on
the vault's net asset value.

## State

The vault stores:

```rust
total_assets: u64,
external_assets: u64,
total_shares: u64,
high_watermark: u64,
performance_fee_bps: u16,
withdrawal_buffer_bps: u16,
max_change_bps: u16,
min_update_interval: i64,
last_update_ts: i64,
```

`total_assets` is the vault's current economic NAV in base-asset units.

`external_assets` is the operator-reported value of deployed or off-chain
positions, also denominated in the base asset.

`total_shares` is the total supply of vault shares tracked by the vault.

`high_watermark` is the highest fee-adjusted share price previously observed.
It is used for performance fee accounting.

`withdrawal_buffer_bps` is the target percentage of total assets to keep idle in
the vault token account for immediate withdrawals.

`max_change_bps` and `min_update_interval` bound NAV updates.

## Total Assets

`total_assets` is derived from observable idle liquidity plus trusted external
value:

```rust
total_assets = idle_assets + external_assets
```

where:

- `idle_assets` is the amount held in the vault token account.
- `external_assets` is the operator-reported value of deployed or off-chain
  positions.

The operator only reports `external_assets`. The program should read the vault
token account balance directly and store the derived `total_assets`.

This keeps the trust boundary narrow: the operator reports only what the program
cannot observe directly.

## NAV Update Flow

Operator calls:

```rust
UpdateTotalAssets { external_assets }
```

The program should:

- verify the caller is the vault operator,
- read the vault token account balance,
- compute `new_total_assets = idle_assets + external_assets`,
- enforce `min_update_interval`,
- enforce `max_change_bps`,
- store `external_assets`, `total_assets`, and `last_update_ts`.

The update should fail if arithmetic overflows or if the NAV change exceeds the
configured guardrail.

## Share Price

Share price is derived from assets and shares:

```rust
share_price = total_assets / total_shares
```

Implementation should use fixed-point integer math rather than floating point.
The exact scale should be defined before deposit, redeem, and fee logic are
implemented.

When `total_shares == 0`, the first depositor initializes the share base. The
current scaffold assumes first deposit is 1:1 with the base asset amount unless
the implementation chooses an explicit initial share scale.

## Deposits

Deposits mint shares at the current share price.

If the vault already has shares:

```rust
shares_to_mint = deposit_amount * total_shares / total_assets
```

If the vault has no shares:

```rust
shares_to_mint = deposit_amount
```

The deposit flow should:

- reject deposits while deposits are paused,
- transfer base assets from the user to the vault token account,
- mint or otherwise account shares to the user,
- increase `total_assets` by `deposit_amount`,
- increase `total_shares` by `shares_to_mint`,
- enforce `min_shares_out`.

Deposits should not change share price except for integer rounding.

## Redeems

Redeems burn shares at the current share price.

```rust
assets_out = shares * total_assets / total_shares
```

The redeem flow should:

- reject new redeems while withdrawals are paused,
- enforce `min_assets_out`,
- burn or otherwise account the user's shares,
- reduce `total_shares` by `shares`,
- reduce `total_assets` by `assets_out`,
- either pay immediately from idle liquidity or create a withdrawal ticket.

Shares are burned before creating a queued withdrawal ticket. That prevents a
user from both keeping shares and claiming queued assets.

## Withdrawal Buffer

`withdrawal_buffer_bps` is a target, not a hard accounting bucket.

The idle target is:

```rust
target_idle_assets = total_assets * withdrawal_buffer_bps / 10_000
```

Operators should manage deployed positions so the vault token account can serve
normal withdrawals. The vault does not store a separate reserved-assets counter;
the vault token account balance is the source of truth for immediate payment
capacity.

## Guardrails

`min_update_interval` prevents rapid repeated NAV updates.

`max_change_bps` caps the magnitude of a single NAV update. A typical check is:

```rust
delta = abs(new_total_assets - old_total_assets)
max_delta = old_total_assets * max_change_bps / 10_000
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

- `total_assets = idle_assets + external_assets` after a successful NAV update.
- `total_shares` changes only when shares are minted or burned.
- Deposits increase both assets and shares proportionally.
- Redeems decrease both assets and shares proportionally.
- The operator cannot directly report idle assets.
- NAV updates must respect `min_update_interval` and `max_change_bps`.
- Withdrawal tickets represent assets already removed from share accounting.
- The vault token account balance is the payment source of truth for immediate
  withdrawals and claims.
