# Design Principles

Roshi is a base-denominated NAV vault.

The program should stay small around the invariants it can enforce on-chain:
role authorization, pause surfaces, account ownership, PDA derivations, custody
movement, share accounting, and explicit NAV update guardrails.

## Base-Denominated Accounting

All vault accounting is denominated in the vault base asset.

Supported non-base deposit assets must be normalized into base atoms before
they affect shares or `total_assets`. Redemptions are base-denominated in the
current design.

Vault shares use fixed 9-decimal accounting. Share decimals do not inherit the
base mint decimals.

Roshi should not compose valuation routes on-chain. Oracle adapters selected by
the vault must already satisfy Roshi's base-denominated price contract.

## NAV Trust Boundary

NAV reporting is an explicit trust boundary.

The program accepts total NAV from `nav_authority`, stores it in base atoms, and
stores `last_report_hash` as a commitment to the private report bundle behind
that value.

This keeps strategy positions, venue balances, counterparties, and private marks
off-chain unless the vault chooses to disclose them. Future verification can
tighten this boundary without changing the core share accounting model.

## Implementation Details

Subaccounts are implementation details for custody and execution isolation.
They are vault-scoped PDA signer authorities, not separate product domains.

Internal accounting should be organized around NAV updates, share math, fee
crystallization, guardrails, and payout configuration. These concepts should
not become a separate user-facing domain unless doing so adds a real invariant.

## Withdrawal Queue

The withdrawal queue is operational.

It exists to handle cases where shares should be burned immediately but base
liquidity is not currently idle in withdrawal custody. The strategist returns
liquidity and the withdrawal authority processes queued tickets through
settlement.

The queue is not a solver market. Roshi does not model withdrawal discounts,
maturity auctions, or deadline markets in the core vault.

## Non-Goals

- No on-chain USD routing, inverse pricing, or composed price legs.
- No base asset PDA.
- No multi-asset redemption path in the current design.
- No on-chain recomputation of full portfolio NAV in v1.
- No queue marketplace.
- No named product domains that only restate internal implementation details.
