# Withdrawal System

Roshi uses a hybrid withdrawal model:

- If the vault has enough idle liquidity, a redeem is paid immediately.
- Otherwise, shares are burned immediately and the user receives a queued
  withdrawal ticket.

The design avoids epoch-derived ticket PDAs. Withdrawal epochs are queue state,
not account address space.

## Goals

- Keep withdrawal account growth bounded.
- Preserve simple user claims.
- Let the vault operator return liquidity asynchronously.
- Avoid storing redundant reserve balances that can drift from the vault token
  account balance.

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

`processed_withdrawal_epoch` is the highest withdrawal epoch that has been
processed by the queue authority and is eligible for claims.

The vault does not store a separate reserved or claimable balance. The vault
token account balance is the source of truth for claim payment capacity.

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

The program computes `assets_owed` from the current share price.

If the vault token account has enough idle liquidity for immediate payment:

- Burn the user's shares.
- Transfer `assets_owed` to the user.
- Do not create or modify a withdrawal ticket.

If the vault does not have enough idle liquidity:

- Burn the user's shares.
- Derive the ticket PDA from `(vault, owner, ticket_index)`.
- Require the selected ticket slot to be empty or otherwise reusable.
- Write `request_epoch = vault.current_withdrawal_epoch`.
- Write `shares_burned` and `assets_owed`.
- Increment `vault.pending_withdrawal_assets` by `assets_owed`.

The user may have up to 256 open queued tickets per vault. Reusing a ticket slot
requires first claiming or clearing the existing ticket in that slot.

## Processing Flow

Queue authority calls:

```rust
ProcessWithdrawals
```

This marks the current queued batch as eligible for claims. At minimum it:

```rust
vault.processed_withdrawal_epoch = vault.current_withdrawal_epoch;
vault.current_withdrawal_epoch += 1;
vault.pending_withdrawal_assets = 0;
```

The instruction should verify the caller is the vault's queue authority.

Processing is an eligibility signal, not a separate escrow movement. The queue
authority is expected to process only once enough liquidity has returned to the
vault token account, but individual claims still pay from the actual token
account balance.

## Claim Flow

User calls:

```rust
Claim
```

and passes the withdrawal ticket account they want to settle.

The program verifies:

```rust
ticket.vault == vault.key()
ticket.owner == user.key()
ticket.assets_owed > 0
ticket.request_epoch <= vault.processed_withdrawal_epoch
ticket.key() == find_ticket(vault, user, ticket.ticket_index)
```

If valid, the program transfers `ticket.assets_owed` from the vault token
account to the user token account.

If the vault token account cannot cover the amount, the token transfer fails
atomically and the ticket remains open.

After a successful transfer, the ticket is closed or cleared so the user can
reuse the same `ticket_index`.

## Invariants

- Shares are burned before a queued withdrawal ticket is created.
- A ticket PDA is bounded by `(vault, owner, ticket_index)`, not by epoch.
- A ticket is claimable only after its `request_epoch` has been processed.
- Claim payment capacity is determined by the vault token account balance.
- The vault does not maintain a separate reserved-assets counter.
- A ticket slot cannot be overwritten while it contains an unclaimed withdrawal.
- Queue processing must be authorized by `vault.queue_authority`.
- Withdrawal pause state should prevent new queued withdrawals, but should not
  block claims for already processed tickets.

## Notes

This model deliberately separates eligibility from payment. Processing an epoch
means the queue authority has made that batch claimable. It does not guarantee a
claim will succeed if liquidity is later moved out of the vault token account.

That tradeoff keeps the state minimal and makes the token account balance the
single source of truth for available payment liquidity.
