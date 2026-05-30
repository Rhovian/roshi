# Withdrawal System

Roshi uses a hybrid withdrawal model:

- If the vault has enough idle liquidity, a redeem is paid immediately.
- Otherwise, shares are burned immediately and the user receives a queued
  withdrawal ticket. The withdrawal authority later processes that ticket all
  the way through settlement.

The design avoids epoch-derived ticket PDAs. Withdrawal epochs are queue state,
not account address space.

## Goals

- Keep withdrawal account growth bounded.
- Keep user interaction simple after a withdrawal request is queued.
- Let the vault strategist return liquidity asynchronously.
- Avoid storing redundant reserve balances that can drift from custody token
  account balances.
- Keep the queue operational rather than market-based.

## Non-Goals

- No withdrawal solver market.
- No discounts.
- No maturity auctions.
- No user-selected deadlines.
- No separate request pricing domain.

## Accounts

### Vault

The vault account stores the queue-level withdrawal state:

```rust
pending_withdrawal_assets: u64,
current_withdrawal_epoch: u64,
processed_withdrawal_epoch: u64,
```

`pending_withdrawal_assets` is the total asset amount requested in the current
unprocessed withdrawal batch.

`current_withdrawal_epoch` is assigned to new queued withdrawal tickets.

`processed_withdrawal_epoch` is the highest withdrawal epoch that has been fully
processed by the withdrawal authority.

The vault does not store a separate reserved or settlement balance. Custody
token account balances are the source of truth for settlement capacity.

### WithdrawalTicket

Each user has a bounded ring of 256 reusable withdrawal ticket PDAs per vault:

```rust
Seeds: [b"ticket", vault, owner, ticket_index]
```

where `ticket_index` is a `u8`.

```rust
WithdrawalTicket {
    vault: Pubkey,
    owner: Pubkey,
    ticket_index: u8,
    request_epoch: u64,
    shares_burned: u64,
    assets_owed: u64,
    bump: u8,
}
```

The ticket stores `request_epoch` as data. The epoch is never part of the PDA
seed.

## Redeem Flow

User calls:

```rust
Redeem {
    ticket_index,
    shares,
    min_assets_out,
}
```

The program computes `assets_owed` from the current share price using checked
integer math:

```text
assets_owed = floor(shares * total_assets / total_shares)
```

See [Accounting Math](./math.md) for the shared helper contract.

If the withdraw subaccount has enough idle liquidity for immediate payment:

- Burn the user's shares.
- Transfer `assets_owed` to the user.
- Do not create or modify a withdrawal ticket.

If the withdraw subaccount does not have enough idle liquidity:

- Burn the user's shares.
- Derive the ticket PDA from `(vault, owner, ticket_index)`.
- Require the selected ticket slot to be empty or otherwise reusable.
- Write `request_epoch = vault.current_withdrawal_epoch`.
- Write `shares_burned` and `assets_owed`.
- Increment `vault.pending_withdrawal_assets` by `assets_owed`.

The user may have up to 256 open queued tickets per vault. Reusing a ticket slot
requires the withdrawal authority to process and clear the existing ticket in
that slot.

## Processing Flow

Withdrawal authority calls:

```rust
ProcessWithdrawals
```

This settles queued withdrawal tickets. The instruction should:

- verify the caller is the vault's withdrawal authority,
- verify the withdraw subaccount custody can pay each supplied ticket,
- transfer each ticket's `assets_owed` to its owner,
- close or clear settled ticket slots,
- advance `processed_withdrawal_epoch` for fully processed batches,
- decrement or clear `pending_withdrawal_assets` for settled tickets.

For each ticket, the program verifies:

```rust
ticket.vault == vault.key()
ticket.assets_owed > 0
ticket.request_epoch <= vault.current_withdrawal_epoch
ticket.key() == find_ticket(vault, user, ticket.ticket_index)
```

Processing is both the eligibility and payment step. The withdrawal authority is
expected to call it only after the strategist has returned enough liquidity to
withdraw custody. If a transfer cannot be paid, the instruction fails atomically
and the ticket remains open.

The withdrawal authority is an operational role. It does not auction withdrawals,
price discounts, or coordinate a solver market. Its job is to settle queued
withdrawals once the vault is ready to satisfy them.

## Invariants

- Shares are burned before a queued withdrawal ticket is created.
- A ticket PDA is bounded by `(vault, owner, ticket_index)`, not by epoch.
- A ticket is settled only by `ProcessWithdrawals`.
- Settlement capacity is determined by custody token account balances.
- The vault does not maintain a separate reserved-assets counter.
- A ticket slot cannot be overwritten while it contains an unsettled withdrawal.
- Withdrawal processing must be authorized by `vault.withdrawal_authority`.
- Withdrawal pause state should prevent new queued withdrawals, but should not
  block authority-driven processing of existing tickets.

## Notes

This model deliberately keeps settlement operational. After a ticket is created,
the user has burned shares and is owed base assets. The withdrawal authority and
strategist are responsible for returning liquidity and processing the withdrawal
through payment.
