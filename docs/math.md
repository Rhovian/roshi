# Accounting Math

Roshi accounting math is integer-only and base-denominated. The program should
never use floating point for NAV, share, oracle, fee, buffer, or guardrail math.

## Units

`base_atoms` are atomic units of the vault base mint. Every vault NAV and
settlement value is denominated in base atoms.

`share_atoms` are atomic units of the vault share supply. Roshi shares always
use 9 decimals:

```rust
SHARE_DECIMALS = 9
```

Share decimals do not inherit the base mint decimals. A USDC-based vault may
have 6 base decimals and 9 share decimals. A SOL-based vault may have 9 base
decimals and 9 share decimals.

Oracle prices are fixed-point values with explicit decimals, quoting one
*whole* token (standard market convention):

```text
price = value / 10^decimals    // quote units per whole token
```

Deposit pricing composes up to two such legs sharing one quote currency —
the asset leg and the base leg — and scales by mint decimals on-chain. A
direct `asset/base` feed is the degenerate case where the base leg is exactly
`1` (the quote currency *is* the base). Roshi never inverts a feed; the base
leg always divides.

`bps` values use the standard basis-point denominator:

```rust
BPS_DENOMINATOR = 10_000
```

## Numeric Policy

Stored token amounts, share amounts, and NAV totals should be `u64` because SPL
token amounts and account state fields are `u64`.

Intermediate arithmetic should use checked `u128` operations:

- multiply in `u128`,
- divide only after multiplication,
- reject division by zero,
- reject overflow,
- reject downcasts that do not fit in `u64`,
- reject zero outputs where the handler requires a meaningful mint or payout.

Ceil division should be implemented with quotient/remainder checks rather than
`product + denominator - 1`, which can overflow.

This keeps state compact while avoiding overflow in realistic vault-scale
products.

## Rounding

Every helper should name its rounding behavior. The default economic posture is
conservative for the vault:

- oracle normalization floors,
- deposit share minting floors,
- redeem asset payout floors,
- withdrawal buffer minimum targets ceil.

User protection belongs in instruction-level minimums where they cannot wedge
deferred settlement, such as deposit `min_shares_out`.

## Primitive Helpers

The common math module should expose small checked helpers rather than open-code
arithmetic in handlers:

```rust
pow10(decimals) -> Result<u128>
mul_div_floor(lhs, rhs, denominator) -> Result<u128>
mul_div_ceil(lhs, rhs, denominator) -> Result<u128>
mul_div_floor_u64(lhs, rhs, denominator) -> Result<u64>
mul_div_ceil_u64(lhs, rhs, denominator) -> Result<u64>
bps_floor(amount, bps) -> Result<u64>
bps_ceil(amount, bps) -> Result<u64>
validate_percentage_bps(bps) -> Result<()>
base_atoms_from_asset_atoms(asset_atoms, asset_price, base_price, asset_decimals, base_decimals)
    -> Result<u64>
virtual_share_offset(base_decimals) -> Result<u128>
shares_for_deposit(base_atoms, total_assets, total_shares, base_decimals) -> Result<u64>
assets_for_shares(shares, total_assets, total_shares, base_decimals) -> Result<u64>
assets_for_redeem(shares, total_assets, total_shares, base_decimals) -> Result<u64>
share_price_from_assets(total_assets, total_shares) -> Result<u64>
performance_fee_for_nav(gross_total_assets, total_shares, high_watermark, performance_fee_bps)
    -> Result<(fee_assets, net_total_assets, high_watermark)>
```

The exact result type should match the program error model used during
implementation.

`bps_floor` and `bps_ceil` only apply the `10_000` denominator. Fields that
represent a percentage cap, such as fees and withdrawal buffer targets, should
use `validate_percentage_bps`.

## Formulas

Normalize a non-base deposit into base atoms through two whole-token price
legs sharing a quote currency (`asset_price` quoting the asset, `base_price`
quoting the base; a direct asset/base feed uses the exact unit base leg
`1 / 10^0`):

```text
base_atoms = floor(
    asset_atoms * asset_price.value * 10^(base_decimals + base_price.decimals)
    / (base_price.value * 10^(asset_decimals + asset_price.decimals))
)
```

The shared powers of ten cancel before multiplication, so the only rounding
is the final floor.

Deposits and redeems price against a virtual position — the ERC-4626
virtual-offset defense against donation/first-deposit share-price inflation:

```text
virtual_shares = 10^(SHARE_DECIMALS - base_decimals)
```

This requires `base_decimals <= SHARE_DECIMALS`, enforced at vault
initialization (a virtual-*asset* offset above 1 would let a full redeem price
above `total_assets`).

Mint shares:

```text
shares_to_mint = floor(base_atoms * (total_shares + virtual_shares) / (total_assets + 1))
```

The same formula covers the first deposit: with `total_shares == 0` and
`total_assets == 0` it reduces to
`base_atoms * 10^SHARE_DECIMALS / 10^base_decimals`, so the initial share scale
is situational to the vault base mint while share decimals stay fixed at 9. For
one whole base unit:

```text
USDC base, 6 decimals: 1_000_000 base atoms -> 1_000_000_000 share atoms
SOL base, 9 decimals: 1_000_000_000 base atoms -> 1_000_000_000 share atoms
```

Redeem shares for base atoms:

```text
assets_out = floor(shares * (total_assets + 1) / (total_shares + virtual_shares))
```

`assets_for_shares` is the raw conversion and may return zero (a dust position
can be worth less than one base atom — withdrawal-ticket strikes price it to
nothing rather than wedge); `assets_for_redeem` is the same conversion with
zero rejected, for immediate-payout paths.

The virtual position makes donation griefing unprofitable: a donor inflating a
later depositor's rounding loss eats ~`virtual_shares` times that loss, because
the virtual position absorbs the donation pro rata. At par (supply/assets equal
to the empty-vault ratio) the offset cancels and pricing is exact; off par it
adds dust-level rounding in the vault's favour.

Compute a withdrawal buffer minimum:

```text
target_idle_assets = ceil(total_assets * withdrawal_buffer_bps / 10_000)
```

Compute a fixed-scale share price:

```text
share_price = floor(total_assets * 10^SHARE_DECIMALS / total_shares)
```

Compute performance fees during NAV reporting:

```text
gross_share_price = floor(gross_total_assets * 10^SHARE_DECIMALS / total_shares)

if high_watermark == 0:
    fee_assets = 0
    net_total_assets = gross_total_assets
    new_high_watermark = gross_share_price

if gross_share_price > high_watermark:
    high_watermark_assets = ceil(high_watermark * total_shares / 10^SHARE_DECIMALS)
    profit_assets = gross_total_assets - high_watermark_assets
    fee_assets = floor(profit_assets * performance_fee_bps / 10_000)
    net_total_assets = gross_total_assets - fee_assets
    new_high_watermark = max(
        high_watermark,
        floor(net_total_assets * 10^SHARE_DECIMALS / total_shares)
    )
```

## Edge Cases

The implementation should explicitly handle:

- `total_shares == 0` and `total_assets == 0` flowing through the same
  virtual-offset formula as every later deposit (no first-deposit special case),
- `base_decimals > SHARE_DECIMALS` rejected before any pricing math runs,
- deposits that round to zero shares,
- redeems that round to zero assets,
- price scales that exceed supported powers of ten,
- stale or invalid oracle values before math helpers are called,
- BPS values above `10_000` where the field represents a percentage cap.

## Test Expectations

Math tests should cover:

- no floating-point usage in public helpers,
- floor and ceil boundary cases,
- overflow and downcast rejection,
- first-deposit examples for 6- and 9-decimal base mints,
- deposit monotonicity,
- redeem monotonicity,
- deposit/redeem round trips never overpay because of rounding,
- share price and performance-fee crystallization examples,
- withdrawal buffer ceiling behavior.

Property tests should cover broad generated input ranges for:

- floor and ceil division bounds,
- BPS floor and ceil ordering,
- deposit share monotonicity,
- redeem asset monotonicity,
- deposit-then-redeem no-overpay behavior,
- redeems never paying more than `total_assets`,
- exact pricing at par and full redeems returning all assets at or below par.
