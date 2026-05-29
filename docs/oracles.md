# Oracles

Roshi oracles are selected to satisfy the vault's base-denominated accounting
requirements.

## Contract

For every supported non-base deposit asset, the oracle value consumed by Roshi
must answer:

```text
how many vault base units is one asset atomic unit worth?
```

Equivalently:

```text
base_value = asset_amount_atoms * base_units_per_asset_atom
```

The exact fixed-point scale is an implementation detail, but the semantic
output is always vault base units.

## Off-Chain Composition

Any routing or composition needed to produce that value belongs outside the
vault program:

- USD legs,
- inverse prices,
- basket marks,
- venue-specific marks,
- strategy-specific discounts,
- private reconciliation.

The program should consume the already-normalized result and enforce freshness,
staleness, and configured move bounds.

## Base Asset

The vault base mint is native to the vault. It does not need a supported asset
PDA and does not need an oracle for deposit-time normalization.

Base deposits use the deposited atomic amount as base value after validating the
mint and custody route.

## Supported Asset Accounts

Non-base assets use vault-scoped `Asset` PDAs:

```text
[b"asset", vault, asset_mint]
```

The `Asset` account records the mint, custody account, oracle configuration,
decimal metadata, deposit limit, enabled flag, and price guardrail fields.

## Invariants

- All non-base deposits normalize to base units before shares are minted.
- The base asset has no `Asset` PDA.
- Oracle values are consumed as direct base-denominated relationships.
- On-chain math should not compose multiple external price feeds.
- A disabled asset cannot be deposited.
