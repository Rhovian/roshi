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

## Atomic-Redeem Exit Fee

`atomic_redeem_fee_bps` charges atomic exits a fee that **stays in the pool**:
the realized proceeds remain in custody, only the net payout leaves and only
the net payout is debited from NAV, so the fee accrues to remaining holders.
Two purposes:

- **Immediacy is a service remaining holders provide** — an atomic exit
  consumes the vault's most liquid inventory while queued redeemers wait.
  The fee compensates the providers.
- **It prices out residual staleness games.** The gate bounds *how* stale an
  atomic exit's price can be; the fee makes exploiting the drift *inside*
  that window unprofitable. Sizing: fee ≥ expected one-report NAV drift
  (recommended default 50 bps). The queue path never pays the fee — patient
  exits are not taxed.

The fee rounds up (in the pool's favor) and `min_output` protects the net
amount the caller actually receives.

## Per-Asset Deposit Caps

`deposit_cap_atoms` bounds each non-base asset's custody inventory:
a deposit rejects when `custody_balance + amount` would exceed the cap. The
cap limits oracle-pricing exposure per asset — how much of the vault's NAV
can enter through any one feed.

It is an **inventory cap, not a flow cap**, by construction: the check reads
the custody balance already in the deposit's account list, so there is no
tracking state to maintain and the cap self-heals as swaps drain custody.
`u64::MAX` means uncapped — explicitly, no zero-means-off magic (a zero cap
blocks all deposits of that asset, equivalent to disabling it). The base
mint is uncapped: it has no oracle leg to bound.

Accepted: donation-griefing — inflating custody to block deposits — is
possible and cheap to undo (the admin raises the cap).

## Mandatory Pyth Confidence Bound

`OracleConfig::validate` rejects an active Pyth leg with
`max_confidence_bps == 0`. An unbounded confidence interval admits an
arbitrarily uncertain, technically-fresh price — staleness checks alone do
not protect against a feed that is current but wide. Only the active leg is
checked; the zeroed inactive half of an `OracleConfig` stays legal. Enforced
through the same validation path every vault/Asset store already runs, so a
misconfigured oracle cannot enter state.

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
**the admin authorizes venues; the strategist only moves funds between
custody and authorized venues.** Registrations are per-vault PDAs over the
destination token account (validated against the base mint, since
`invest_external` only moves base out); revocation closes the PDA, and
`invest_external` rejects unregistered or revoked destinations.
`return_external` is inbound and stays unrestricted.

## Withdrawal Liveness Escape

`cancel_grace_slots` re-opens cancellation for a strike-eligible but still
*unstruck* ticket once the grace elapses (fact 4: a dead withdrawal authority
must not trap users). The redeemer re-enters the pool with their original
shares. Chosen as a vault config rather than a program constant (decided
2026-06-12) — the admin tunes it to the vault's operational cadence; 0
disables the escape.

Deliberately deferred: a *struck* ticket (payout already fixed) can still be
trapped by a dead key. Permissionless settlement after a grace was considered
and deferred — documented as an accepted risk below.

## Oracle-Bounded Swap Slippage

`max_swap_slippage_bps` requires every swap's realized output value to cover
its realized input value minus the tolerance, both valued through the same
oracle path deposits use. The caller's `max_in`/`min_out` bound *amounts*; a
compromised swap authority sets them loose. The value bound is the control
that survives key compromise: an authorized route that leaks NAV (output
worth less than input) rejects regardless of what the authority claims.

The vault base-oracle leg is read **once per swap** and shared by both
endpoint valuations: a pull-oracle feed can have several verified updates
inside its freshness window, and letting each side submit its own copy would
let one comparison price the same feed two different ways (low for the
output, high for the input), widening the bound by the feed's intra-window
drift.

Settled posture (2026-06-12): **endpoints must price.** Both endpoint custody
mints must be the base mint or a registered Asset (routed legs included), or
the swap rejects (`UnpriceableSwapLeg`). Whatever happens inside the single
authorized CPI — aggregator multi-hop through arbitrary intermediates — stays
opaque; route competitiveness comes from the venue, not from weakening the
bound. A deposit-disabled Asset still prices (the `enabled` flag gates
deposits, not valuation).

## Token-2022 Extension Allowlist

Mint verification walks the Token-2022 extension TLV and allows exactly two
extension types: `MetadataPointer` and `TokenMetadata` (display only).
Everything else — transfer fees, transfer hooks, permanent delegates, close
authorities, confidential transfers, interest, pausability, unknown future
types — is rejected. Allowlist, not blocklist: an extension Roshi has never
heard of is treated as hostile until proven benign, because fee-on-transfer
alone silently breaks `total_assets` accounting (amount sent != amount
received).

Registration-time checking is sound: every dangerous mint extension must be
initialized before `InitializeMint`, so a mint that passes at registration
cannot grow one later. The one post-creation growth case is `TokenMetadata`
(a realloc) — benign, allowed, and the reason no code path may assume a fixed
mint account length.

## Share-Mint Metadata

Share mints carry Metaplex Token Metadata set by the admin-gated
`SetShareMetadata` (bare mints render as "Unknown Token" — bad UX and a
phishing surface). Metaplex over a Token-2022 migration because share tokens
must stay maximally composable classic SPL — integrators treat Token-2022
with the same extension suspicion Roshi itself applies — and Metaplex costs
one instruction instead of touching every share flow. The vault PDA is the
metadata update authority, so renames go through the same admin instruction
and nothing outside the program can mutate it. Display only: no economic
invariant may depend on metadata.

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
- **Struck-but-unsettled tickets can be trapped by a dead withdrawal
  authority.** Permissionless settlement was considered and deferred; the
  cancel grace covers unstruck tickets only.
- **Routed-leg quote consistency is operator config.** Two oracle legs must
  share a quote currency; the program cannot verify they do. Using X/USD legs
  on a stable base assumes the USD≈stable peg.
- **Donation-griefing of deposit caps**: inflating custody to block deposits
  is accepted; the admin raises the cap.

## Prior Art

The lazy linear profit unlock converges with Yearn v3's mechanism, which has
years of adversarial mainnet exposure without the unlock itself being an
exploit. The convergence is evidence, not lineage: the design here is derived
from the threat model above, and deviates where Roshi's accounting differs —
adaptive window instead of fixed, asset-side lock instead of share dilution,
and reporter bounds (gain bound, rate limit, staleness gate) that an
on-chain-measurable-profit system does not need.

## Status

Every control in this document is enforced: profit unlock, staleness gate
(atomic redeem), NAV gain bound, report rate limit, atomic-redeem exit fee,
per-asset deposit caps, mandatory Pyth confidence bound, fee writedown,
destination registry (including `invest_external` gating), cancel grace,
oracle-bounded swap slippage, the Token-2022 extension allowlist, and
share-mint metadata.
