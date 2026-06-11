# Oracles

Roshi oracles are selected to satisfy the vault's base-denominated accounting
requirements.

## Contract

For every supported non-base deposit asset, the oracle value consumed by Roshi
must answer:

```text
how many vault base atoms is one asset atom worth?
```

Equivalently:

```text
base_atoms = floor(asset_atoms * price_value / 10^price_decimals)
```

Where:

```text
price_value / 10^price_decimals = base_atoms_per_asset_atom
```

The semantic output is always a direct `asset/base` relationship.

## Direct Asset/Base Only

Any routing or composition needed to produce that value belongs outside the
vault program. Roshi does not consume:

- USD legs,
- inverse prices,
- basket marks,
- venue-specific marks,
- strategy-specific discounts,
- private reconciliation.

An off-chain system may source or compute a mark however the vault operator
chooses, including from venues or reports that use another quote convention.
Before Roshi uses that mark, the configured oracle account must already encode
the final direct `asset/base` fixed-point value. The program should then enforce
ownership, feed identity, freshness, staleness, confidence, and configured move
bounds.

## Supported Providers

### Switchboard On-Demand

Switchboard configs pin the quote account, queue account, feed id, output
decimal scale, and max quote age in slots. State-changing handlers should use
the verified reader so queue, slot-hash, instruction-sysvar, and max-age checks
run before a value is accepted.

The verified reader also pins the depositor-supplied slot-hashes and
instructions sysvars to their canonical ids at the Roshi boundary, before
anything is handed to Switchboard's `QuoteVerifier` — the library validates
them internally today, but Roshi's account contract does not depend on that.

### Pyth Pull Oracle

Pyth configs pin the feed id, output decimal scale, max update age in seconds,
and optional confidence guardrail in basis points. The on-chain reader accepts
Pyth `PriceUpdateV2` accounts owned by the Pyth Solana Receiver program.

For fixed price-feed accounts, clients pass the derived feed account address to
Roshi. For ephemeral price-update accounts, clients fetch updates from Hermes,
post them with the Pyth Solana Receiver, then pass the resulting update account
to the Roshi instruction that consumes the oracle value.

The config may additionally pin one specific price update account by address
(`pin_price_update_account`), restricting pricing to e.g. a sponsored feed
account. Unpinned (the default), the depositor chooses which verified in-range
update for the configured feed to submit — intentional for the pull-oracle
model, with staleness and spread bounded by `max_age_seconds` and
`max_confidence_bps`.

Roshi requires the configured feed id, full Pyth verification, a positive price,
freshness within `max_age_seconds`, and confidence within
`max_confidence_bps` when that guardrail is nonzero. Pyth's exponent is scaled
to the configured `price_decimals` before returning the base-denominated
`OraclePrice`.

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

- All non-base deposits normalize to base atoms before shares are minted.
- The base asset has no `Asset` PDA.
- Oracle values are consumed as direct `asset/base` relationships.
- On-chain math must not compose, invert, or route external price feeds.
- A disabled asset cannot be deposited.
