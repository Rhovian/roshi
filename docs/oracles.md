# Oracles

Roshi oracles are selected to satisfy the vault's base-denominated accounting
requirements.

## Contract

Oracle feeds quote one *whole* token in fixed point — the standard market
convention every Pyth and Switchboard feed already publishes:

```text
price = value / 10^decimals    // quote units per whole token
```

Roshi scales whole-token prices into atom terms on-chain using the asset and
base mint decimals; feeds never need to encode mint decimals themselves.

Each supported asset prices deposits in one of two modes:

### Direct

The asset's feed quotes whole asset tokens directly in whole base tokens
(e.g. an STETH/ETH feed for an ETH-base vault):

```text
base_atoms = floor(asset_atoms * price.value * 10^base_decimals
                 / 10^(asset_decimals + price.decimals))
```

### Routed

The asset's feed and the vault's `base_oracle` share one quote currency
(typically USD): the asset leg quotes asset/QUOTE, the base leg quotes
BASE/QUOTE, and the program composes asset/base as their ratio:

```text
base_atoms = floor(
    asset_atoms * asset_price.value * 10^(base_decimals + base_price.decimals)
    / (base_price.value * 10^(asset_decimals + asset_price.decimals))
)
```

This is how arbitrary token pairs price against arbitrary bases using only
standard feeds — e.g. SOL deposits into a USDC-base vault via SOL/USD and
USDC/USD. Roshi never inverts a feed; the base leg always divides.

That both legs really share a quote currency is a configuration fact the
program cannot observe — pairing an X/USD asset leg with a BASE/EUR base leg
misprices silently. It is the vault operator's contract, like feed identity
itself. Beyond the two supported legs, Roshi does not consume:

- basket marks,
- venue-specific marks,
- strategy-specific discounts,
- private reconciliation.

The program enforces ownership, feed identity, freshness, staleness, and
confidence per leg.

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
and a confidence guardrail in basis points. The guardrail is mandatory for an
active Pyth leg — `OracleConfig::validate` rejects `max_confidence_bps == 0`,
since an unbounded confidence interval admits an arbitrarily uncertain,
technically-fresh price. The on-chain reader accepts Pyth `PriceUpdateV2`
accounts owned by the Pyth Solana Receiver program.

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
mint and custody route. The vault-level `base_oracle` exists to price *other*
assets in routed mode; base deposits themselves never read it.

## Supported Asset Accounts

Non-base assets use vault-scoped `Asset` PDAs:

```text
[b"asset", vault, asset_mint]
```

The `Asset` account records the mint, oracle configuration, mint decimals,
enabled flag, and pricing mode (direct or routed). Custody is the deposit
sub-account's ATA for the mint, derived rather than stored.

The vault's `base_oracle` config supplies the BASE/QUOTE leg for every routed
asset. Routed deposits append the base oracle's accounts after the asset
oracle's accounts.

## Invariants

- All non-base deposits normalize to base atoms before shares are minted,
  scaled on-chain by asset and base mint decimals.
- The base asset has no `Asset` PDA.
- Oracle legs are whole-token prices; direct mode consumes one asset/base
  leg, routed mode composes asset/QUOTE over BASE/QUOTE.
- On-chain math never inverts a feed and composes at most the two configured
  legs.
- A disabled asset cannot be deposited.
