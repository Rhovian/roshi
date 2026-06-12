# Design Principles

Roshi is a base-denominated NAV vault.

Roshi is experimental and unaudited software. It is provided as-is, without
warranties or liability. Operators and integrators are responsible for their own
review, testing, and risk assessment before any production use.

The program should stay small around the invariants it can enforce on-chain:
role authorization, pause surfaces, account ownership, PDA derivations, custody
movement, deposit access checks, share accounting, and explicit NAV reporting.

## Base-Denominated Accounting

All vault accounting is denominated in the vault base asset.

Supported non-base deposit assets must be normalized into base atoms before
they affect shares or `total_assets`. Redemptions are base-denominated in the
current design.

Vault shares use fixed 9-decimal accounting. Share decimals do not inherit the
base mint decimals.

Roshi should not compose valuation routes on-chain. Oracle adapters selected by
the vault must already satisfy Roshi's base-denominated price contract.

## Controls And Access

Vaults can be public or private.

Private vaults gate deposits with a Merkle proof against a vault-level access
root. The proof is supplied on the deposit instruction and is not stored.
Access gating does not block redemptions or withdrawal processing for existing
share owners. Roles, pause flags, and access mode are control-plane surfaces;
see [Controls](./controls.md).

## NAV Trust Boundary

NAV reporting is an explicit trust boundary.

The program accepts total NAV from `nav_authority`, stores it in base atoms, and
stores `last_report_hash` as a commitment to the private report bundle behind
that value.

This keeps strategy positions, venue balances, counterparties, and private marks
off-chain unless the vault chooses to disclose them. Future verification can
tighten this boundary without changing the core share accounting model.

The boundary is bounded, not blind: reported gains drip into the share price
(profit unlock), upward price moves are capped per report and rate-limited,
and atomic exits reject stale reports. The threat model and the rationale for
each control live in [Economic Controls](./economic-controls.md).

## Implementation Details

Subaccounts are implementation details for custody and execution isolation.
They are vault-scoped PDA signer authorities, not separate product domains.
See [Execution](./execution.md) for subaccount signer behavior.

Internal accounting should be organized around NAV updates, share math, fee
crystallization, and payout configuration. These concepts should not become a
separate user-facing domain unless doing so adds a real invariant.

## Withdrawal Queue

The withdrawal queue is operational.

Redemptions always burn shares immediately and create queued withdrawal tickets.
The strategist returns liquidity to withdrawal custody, and the withdrawal
authority processes queued tickets through settlement.

The queue is not a solver market. Roshi does not model withdrawal discounts,
maturity auctions, or deadline markets in the core vault.

## Non-Goals

- No on-chain USD routing, inverse pricing, or composed price legs.
- No base asset PDA.
- No multi-asset redemption path in the current design.
- No on-chain recomputation of full portfolio NAV in v1.
- No queue marketplace.
- No named product domains that only restate internal implementation details.
