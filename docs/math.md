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

`price_value` is an oracle fixed-point value with `price_decimals` decimals.
For Roshi, the value must mean:

```text
base_atoms_per_asset_atom = price_value / 10^price_decimals
```

Roshi does not consume USD, inverse, or routed price semantics in accounting
math. Any source process that produces a mark must present the final value as a
direct `asset/base` relationship before the program uses it.

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
- max NAV delta guardrails floor,
- withdrawal buffer minimum targets ceil.

User protection belongs in instruction-level minimums such as
`min_shares_out` and `min_assets_out`.

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
base_atoms_from_asset_atoms(asset_atoms, price_value, price_decimals) -> Result<u64>
initial_shares_from_base_atoms(base_atoms, base_decimals) -> Result<u64>
shares_for_deposit(base_atoms, total_assets, total_shares) -> Result<u64>
assets_for_redeem(shares, total_assets, total_shares) -> Result<u64>
nav_delta_within_bps(old_total_assets, new_total_assets, max_change_bps) -> Result<bool>
```

The exact result type should match the program error model used during
implementation.

`bps_floor` and `bps_ceil` only apply the `10_000` denominator. They should not
reject values above `10_000`, because basis points are a scale and some
guardrail fields may intentionally allow moves above 100%. Fields that represent
a percentage cap, such as fees and withdrawal buffer targets, should use
`validate_percentage_bps`.

## Formulas

Normalize a non-base deposit into base atoms:

```text
base_atoms = floor(asset_atoms * price_value / 10^price_decimals)
```

Mint initial shares when `total_shares == 0`:

```text
initial_shares = floor(base_atoms * 10^SHARE_DECIMALS / 10^base_decimals)
```

This makes the initial share scale situational to the vault base mint while
keeping share decimals fixed at 9. For one whole base unit:

```text
USDC base, 6 decimals: 1_000_000 base atoms -> 1_000_000_000 share atoms
SOL base, 9 decimals: 1_000_000_000 base atoms -> 1_000_000_000 share atoms
```

Mint shares into an existing vault:

```text
shares_to_mint = floor(base_atoms * total_shares / total_assets)
```

Redeem shares for base atoms:

```text
assets_out = floor(shares * total_assets / total_shares)
```

Compute a withdrawal buffer minimum:

```text
target_idle_assets = ceil(total_assets * withdrawal_buffer_bps / 10_000)
```

Check a NAV update guardrail:

```text
delta = abs(new_total_assets - old_total_assets)
max_delta = floor(old_total_assets * max_change_bps / 10_000)
delta <= max_delta
```

## Edge Cases

The implementation should explicitly handle:

- `total_shares == 0` and `total_assets == 0` for first deposit,
- `total_shares > 0` with `total_assets == 0` as an invalid accounting state
  unless a deliberate recovery path exists,
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
- NAV guardrail boundary values,
- withdrawal buffer ceiling behavior.

Property tests should cover broad generated input ranges for:

- floor and ceil division bounds,
- BPS floor and ceil ordering,
- deposit share monotonicity,
- redeem asset monotonicity,
- deposit-then-redeem no-overpay behavior,
- full redeem returning all assets,
- NAV guardrail equivalence to the documented formula.
