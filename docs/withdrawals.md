# Withdrawal System

Roshi uses a queued withdrawal model:

Shares are burned immediately and the user receives a queued withdrawal ticket.
The withdrawal authority later processes that ticket all the way through
settlement after the strategist has returned enough base liquidity to withdrawal
custody.

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

The vault account stores aggregate pending withdrawal state:

```rust
pending_withdrawal_assets: u64,
```

`pending_withdrawal_assets` is the total base-asset amount owed across open
withdrawal tickets.

The vault does not store a separate reserved or settlement balance. Custody
token account balances are the source of truth for settlement capacity.

### WithdrawalTicket

Each recipient token account has a bounded ring of 256 reusable withdrawal
ticket PDAs per vault:

```rust
Seeds: [b"ticket", vault, recipient_token_account, ticket_index]
```

where `ticket_index` is a `u8`.

```rust
WithdrawalTicket {
    vault: Pubkey,
    owner: Pubkey,
    recipient_token_account: Pubkey,
    ticket_index: u8,
    shares_burned: u64,
    assets_owed: u64,
    bump: u8,
}
```

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

The redeem flow:

- burns the user's shares,
- derives the ticket PDA from `(vault, recipient_token_account, ticket_index)`,
- requires the selected ticket slot to be empty or otherwise reusable,
- writes `owner`, `recipient_token_account`, `shares_burned`, and `assets_owed`,
- increments `vault.pending_withdrawal_assets` by `assets_owed`.

A recipient token account may have up to 256 open queued tickets per vault.
Reusing a ticket slot requires the withdrawal authority to process and clear the
existing ticket in that slot.

## Processing Flow

Withdrawal authority calls:

```rust
ProcessWithdrawals
```

This settles queued withdrawal tickets. The instruction should:

- verify the caller is the vault's withdrawal authority,
- verify the withdraw subaccount custody can pay each supplied ticket,
- transfer each ticket's `assets_owed` to its recorded recipient token account,
- close or clear settled ticket slots,
- decrement or clear `pending_withdrawal_assets` for settled tickets.

For each ticket, the program verifies:

```rust
ticket.vault == vault.key()
ticket.owner == owner.key()
ticket.recipient_token_account == destination.key()
ticket.assets_owed > 0
ticket.key() == find_ticket(vault, destination, ticket.ticket_index)
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
- A ticket PDA is bounded by `(vault, recipient_token_account, ticket_index)`.
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
