# RBAC And Pausing

Roshi stores named role authorities on each vault.

```rust
admin: Pubkey,
strategist: Pubkey,
nav_authority: Pubkey,
withdrawal_authority: Pubkey,
```

## Roles

`admin` controls vault configuration:

- update roles,
- update pause flags,
- configure supported assets,
- authorize or revoke actions,
- choose default deposit and withdrawal subaccounts.

`strategist` executes authorized strategy CPIs through `manage` and
`manage_batch`.

`nav_authority` submits total NAV reports and report commitments.

`withdrawal_authority` processes queued withdrawals through settlement.

These roles may be the same signer at launch, but the protocol models them
separately so operations can move to distinct wallets, bots, or multisigs
without changing account layout.

## Pause Flags

The vault has separate pause flags for separate risk surfaces:

```rust
deposits_paused: bool,
withdrawals_paused: bool,
manage_paused: bool,
```

`deposits_paused` blocks new deposits.

`withdrawals_paused` blocks new redemptions or withdrawal requests. It should
not block authority-driven processing of already queued withdrawals.

`manage_paused` blocks strategist CPI execution across all subaccounts.

NAV reports are not separately paused in the current scaffold. If the
`nav_authority` is compromised, the admin can rotate it and use NAV guardrails
to limit accepted report movement.

Pause flags have a dedicated instruction surface:

```rust
SetPauseFlags {
    deposits_paused,
    withdrawals_paused,
    manage_paused,
}
```

`SetPauseFlags` should be admin-only. It is intentionally narrower than full
vault config replacement so emergency pause changes do not require resubmitting
role, fee, guardrail, or subaccount configuration.

## Invariants

- Admin-only instructions must verify `vault.admin`.
- Manage instructions must verify `vault.strategist`.
- NAV update instructions must verify `vault.nav_authority`.
- Withdrawal processing must verify `vault.withdrawal_authority`.
- Pause flags gate behavior, not role identity.
