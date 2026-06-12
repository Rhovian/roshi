# Economic Controls

Design rationale for the economic hardening controls: the threat model they
answer, the mechanism each one uses, and the trade-offs that were decided
deliberately. Mechanics live in [Accounting](./accounting.md); the config
surface lives in [Controls](./controls.md). This document records *why*.

## Threat Model

Four facts drive every control:

1. **Discrete NAV reports create a front-runnable price step.** Between
   reports the on-chain price is frozen while the real portfolio moves. A
   depositor entering just before a gain report captures the jump; the async
   redeem queue does **not** protect the other direction — a ticket filed at
   epoch N−1 strikes at epoch N, jump included — and an atomic redeem exits at
   the current price instantly.
2. **Losses and gains are not symmetric.** A smoothed loss is an exit subsidy
   for informed redeemers: anyone who knows the loss is coming leaves at the
   stale-high price and dumps the loss on whoever stays. Honest bad news must
   land in one report, immediately.
3. **A compromised NAV authority profits from *upward* moves only.** The
   attack is inflate-then-drain: report a higher NAV, exit at the inflated
   price. Downward manipulation gives the attacker nothing (and bounding it
   would lengthen the stale window an atomic-exit attacker uses).
4. **Authorities fail independently.** Strategist, swap, NAV, and withdrawal
   authorities are separate keys that can each be compromised or lost. Every
   authority power should be either admin-pinned or bounded, and no user funds
   may be trappable by a dead key.

## Profit Unlock (gains drip, losses land instantly)

The core anti-frontrun mechanism. A reported gain does not reach the share
price at once: it is locked and drips in linearly. Share pricing everywhere —
deposit mint, redeem dust guard, ticket strike, atomic-redeem entitlement —
uses **effective NAV**:

```text
effective_total_assets(now) = total_assets - remaining_locked_profit(now)
```

**Adaptive window.** Each gain drips over `min(elapsed_since_last_report,
max_unlock_duration_secs)` — the span it was actually earned in, clamped. A
fixed window (prior art uses one) over-smooths frequent reports and
under-smooths rare ones; the adaptive window means a front-runner's capture
rate ≈ the vault's organic earning rate, with no report-cadence commitment and
no minimum clamp (rapid reports carry proportionally small gains).

**Losses recognize instantly**, absorbed by the locked remainder first:
`locked_profit = 0` together with the lower `total_assets` means effective NAV
declines but never jumps up. This is fact 2 made mechanical.

**Nothing cranks the drip.** The vault stores only `(locked_profit, start_ts,
end_ts)` — a line — and every reader interpolates it against the clock sysvar
at read time. No keeper, no crank instruction, no liveness dependency: an
untouched vault has fully dripped by the time the next reader shows up.
Off-chain readers compute the identical value from account data and wall-clock
time.

**Asset-side, not share-side.** The lock is a subtraction from pricing NAV,
not shares minted to the vault and burned down (the share-dilution variant
used elsewhere). Share-side would interact badly with three Roshi mechanisms:
the economic share supply already carries `requested_withdrawal_shares` for
the redeem queue, the performance-fee high watermark ratchets on share price,
and every supply read would need a vault-owned-shares adjustment. One
subtraction replaces three interactions.

**Forfeiture socializes to whoever stays.** A mid-drip redeemer is paid at
effective NAV and forfeits their slice of the still-locked profit, which
accrues pro-rata to remaining holders. Deliberate: the protocol favors
participants who stay.

**Fees and the high watermark stay on report-time gross.** The gain was
fee-charged when first reported; charging it again as it drips would
double-count. The HWM ratchets on the full net price for the same reason.

**Debits re-anchor the line.** Strikes and atomic redeems shrink
`total_assets` mid-drip, which could leave the stored `locked_profit` above
the new total even though the *remaining* drip is not. State validation has no
clock, so the invariant `locked_profit <= total_assets` must hold statically:
every effective-priced debit rewrites the window as `(remaining(now), now,
end)` — the same line, re-anchored (exact up to one atom of floor rounding).

## Staleness Gate — atomic redeem only

`max_report_age_secs` rejects **atomic redeems** once the last report is older
than the configured age. An atomic exit prices instantly, so a stale-high
price lets an informed redeemer escape an incurred-but-unreported loss and
dump it entirely on remaining holders — a pure externality, and the one place
the gate is irreplaceable.

**Deposits are never staleness-gated** (decided 2026-06-12). The deposit-side
externality does not exist:

- *Stale-low entry* (unreported gain): capture is already bounded to ≈ the
  organic rate by the profit unlock, and the catch-up report itself is rate-
  limited by the gain bound × report interval. Nothing left for a gate to do.
- *Stale-high entry* (unreported loss): harms only the depositor — existing
  holders benefit. Informed depositors self-protect with `min_shares_out`
  (computed off their own NAV view, it fails the transaction); the admin
  `deposits_paused` flag covers a genuinely dark vault. Coupling deposit
  availability to NAV-reporter uptime would pay a real liveness cost on every
  infra hiccup for protection other controls already provide.

Queued redeems are also never gated: they are priced later, at strike, so
staleness cannot be exploited through them. Pre-first-report vaults are exempt
(pricing is exactly par via the virtual offset).

## NAV Move Bound — asymmetric, plus rate limit

`max_nav_gain_bps` caps how far one report may move the **net share price up**
vs. the stored pre-report price. There is deliberately **no downward bound**
(facts 2 and 3): honest losses land whole, and a down-bound would stretch the
stale window during a drawdown — actively helping the atomic-exit attacker.

The bound alone has a hole: a compromised authority chains many small,
individually in-bound reports. `min_report_interval_secs` closes it; together
they give a hard ceiling on attacker-driven inflation of
`max_nav_gain_bps` per `min_report_interval_secs`.

**A bounced over-bound report is not an error state.** The authority reports
the capped amount now and rolls the remainder into subsequent reports, each
spaced by the interval and each re-locking into the drip. The bound doubles as
a rate limiter on honest catch-up after a reporting gap. Report-authority
runbooks should treat `NavGainExceedsBound` as "split the report", not as a
failure.

The bound is computed against stored `total_assets` (recognized NAV), not
effective NAV — it governs report-to-report movement, and using effective
would double-count the in-flight drip. It is skipped when the share supply or
the stored price is zero, so post-total-loss recovery cannot wedge.

## Fees-Only Insolvency Writedown

`WriteDownFees` lets the admin forgive accrued fee liability — no token
movement, gross NAV untouched. It exists for exactly one state: losses ate
into the fee cushion (`gross < fees_payable + pending_withdrawal_assets`),
which wedges `report_nav`. Writing fees down shrinks liabilities and unwedges
the report path.

Struck withdrawal tickets remain **inviolable** — a loss deeper than the fee
cushion leaves the vault wedged *by design*. The alternative (haircutting
struck tickets) would retroactively reprice claims that were already fixed,
which is precisely what the strike mechanism promises never happens. Recovery
from a deeper insolvency is operational: pause deposits, unwind manually.

## External Destination Registry

`invest_external`'s destination was the one strategist power not admin-pinned:
a free-form destination is a custody-exfiltration path on strategist key
compromise (fact 4). The registry mirrors the Asset/action-hash philosophy —
**the admin authorizes venues; the strategist will only move funds between
custody and authorized venues.** Registrations are per-vault PDAs over the
destination token account (validated against the base mint, since
`invest_external` only moves base out); revocation closes the PDA.
`return_external` is inbound and stays unrestricted. The registry
instructions exist today; `invest_external` does not yet require a
registered destination — that enforcement ships with the remaining hardening
work (see Status).

## Trust Posture and Accepted Risks

V1 trusts the NAV authority *within the bounds above*; the controls shrink the
blast radius of a compromised key from "drain the vault" to "bounded drift per
interval", they do not remove the trust boundary.

Accepted risks, documented rather than mitigated:

- **Deposits before a markdown.** An uninformed depositor can buy just before
  a loss report (the same exposure as buying any fund at last-published NAV).
  Informed depositors have `min_shares_out`; front-ends should surface
  `last_update_ts`.
- **Global high watermark taxes above-HWM entrants.** Per-account HWMs are out
  of scope.
- **Donations accrue performance fee.** The virtual offset bounds the related
  share-price attack; donated value itself is treated as profit.
- **Insolvency beyond the fee cushion wedges by design** (see Writedown).
- **Routed-leg quote consistency is operator config.** Two oracle legs must
  share a quote currency; the program cannot verify they do. Using X/USD legs
  on a stable base assumes the USD≈stable peg.
- **Donation-griefing of deposit caps** (when caps land): inflating custody to
  block deposits is accepted; the admin raises the cap.

## Prior Art

The lazy linear profit unlock converges with Yearn v3's mechanism, which has
years of adversarial mainnet exposure without the unlock itself being an
exploit. The convergence is evidence, not lineage: the design here is derived
from the threat model above, and deviates where Roshi's accounting differs —
adaptive window instead of fixed, asset-side lock instead of share dilution,
and reporter bounds (gain bound, rate limit, staleness gate) that an
on-chain-measurable-profit system does not need.

## Status

Enforced today: profit unlock, staleness gate (atomic redeem), NAV gain bound,
report rate limit, fee writedown, destination registry instructions.

Config fields present but not yet enforced (mechanisms land with their plan
sections): `atomic_redeem_fee_bps` (exit fee paid to the pool),
`deposit_cap_atoms` (per-asset inventory cap), `max_swap_slippage_bps`
(oracle-bounded swap slippage on priceable endpoints), `cancel_grace_slots`
(withdrawal-authority liveness escape), and `invest_external` does not yet
require a registered destination.
