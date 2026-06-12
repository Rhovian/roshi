# Controls

Roshi separates control-plane permissions from economic accounting and
strategy execution.

## Roles

Each vault stores named authorities:

```rust
admin: Pubkey,
strategist: Pubkey,
swap_authority: Pubkey,
nav_authority: Pubkey,
withdrawal_authority: Pubkey,
```

`admin` controls vault configuration:

- transfer vault admin authority,
- update operational role authorities,
- update pause flags,
- update private/public access mode and access Merkle root,
- configure supported assets and economic controls,
- authorize or revoke actions,
- register or revoke external investment destinations,
- choose default deposit and withdrawal subaccounts,
- collect accrued fees, and write down fee liability,
- set the share mint's display metadata (Metaplex).

`strategist` executes authorized strategy CPIs through `manage` and
`manage_batch`.

`swap_authority` executes pre-authorized swap CPIs between vault custodies.

`nav_authority` submits gross NAV reports and report commitments.

`withdrawal_authority` settles queued withdrawal tickets.

These roles may be the same signer at launch, but the protocol models them
separately so operations can move to distinct wallets, bots, or multisigs
without changing account layout.

## Authority Rotation

Program and vault authority transfers have dedicated instruction surfaces:

```rust
TransferProgramAuthority {
    new_authority,
}

TransferVaultAuthority {
    new_authority,
}
```

Operational roles have dedicated setters:

```rust
SetStrategist {
    strategist,
}

SetNavAuthority {
    nav_authority,
}

SetWithdrawalAuthority {
    withdrawal_authority,
}
```

The vault PDA is derived from `[b"vault", tag, base_mint]`. It does not include
the admin wallet, so admin transfers do not change the vault address.

## Pause Flags

The vault has separate pause flags:

```rust
deposits_paused: bool,
withdrawals_paused: bool,
manage_paused: bool,
```

`deposits_paused` blocks new deposits.

`withdrawals_paused` blocks new redemptions and withdrawal-ticket creation. It
does not block authority-driven settlement of already queued withdrawals.

`manage_paused` blocks strategist CPI execution across all subaccounts.

NAV reports are not separately paused. If `nav_authority` is compromised, the
admin can rotate it.

Pause updates use:

```rust
SetPauseFlags {
    deposits_paused,
    withdrawals_paused,
    manage_paused,
}
```

## Economic Controls

The vault embeds an admin-configured `VaultControls` block. Zero disables a
control, so the all-zeros default is "every control off". Set at
`InitializeVault` and replaced atomically by `UpdateVaultConfig`.

```rust
VaultControls {
    max_unlock_duration_secs: u32, // profit-unlock window clamp
    max_report_age_secs: u32,      // staleness gate (atomic redeem only)
    min_report_interval_secs: u32, // report rate limit
    cancel_grace_slots: u32,       // ticket-cancel liveness escape
    max_nav_gain_bps: u16,         // upward NAV move bound per report
    atomic_redeem_fee_bps: u16,    // atomic-exit fee, retained by the pool
    max_swap_slippage_bps: u16,    // oracle-bounded swap slippage
}
```

Recommended posture: `max_unlock_duration_secs` around 7 days (604_800);
`min_report_interval_secs` and `max_nav_gain_bps` sized so their ratio caps
NAV drift at the strategy's plausible organic rate; `max_report_age_secs` a
small multiple of the expected reporting cadence; `atomic_redeem_fee_bps`
around 50 — at or above the expected one-report NAV drift, so escaping an
incurred-but-unreported loss through the atomic path is unprofitable (the
queue path never pays the fee). `max_nav_gain_bps` may exceed 10_000 (a
bound above +100% is meaningful); the two fee/slippage fields are
percentage-capped at 10_000.

Deposits are never staleness-gated, and there is deliberately no downward NAV
bound. The threat model and rationale for every control live in
[Economic Controls](./economic-controls.md); the mechanics live in
[Accounting](./accounting.md).

## External Destinations

`invest_external` destinations are admin-authorized venues, registered as
vault-scoped PDAs over the destination base token account:

```text
[b"external_destination", vault, destination_token_account]
```

`RegisterExternalDestination` (admin-gated, destination validated as a
base-mint token account) creates the registration;
`RevokeExternalDestination` closes it back to the admin. The admin authorizes
venues; the strategist only moves funds between custody and authorized
venues. `return_external` is inbound and unrestricted.
`invest_external` requires the destination's registration account and rejects
unregistered (or revoked) destinations.

## Vault Access

Vaults can be public or private.

Public vaults accept deposits from any valid depositor, subject to pause flags,
asset configuration, oracle checks, and share math.

Private vaults require depositors to prove membership in an admin-controlled
Merkle allowlist of pubkeys. Access control gates new deposits only. It does
not prevent existing share owners from redeeming or claiming queued withdrawals.

The vault stores:

```rust
private: bool,
access_merkle_root: [u8; 32],
```

`private = false` means the Merkle root is ignored.

`private = true` means deposits must include a valid proof for the depositor
signer against `access_merkle_root`. If `private = true` and
`access_merkle_root = [0; 32]`, the vault is closed to new deposits.

Access updates use:

```rust
SetVaultAccess {
    private,
    access_merkle_root,
}
```

This lets the admin switch between public and private mode, rotate the allowlist
root, or disable new private deposits by setting a zero root while private mode
is enabled.

## Access Proofs

Deposits carry:

```rust
access_proof: Vec<[u8; 32]>
```

Proofs are capped at 32 sibling hashes. This is a proof-depth cap, not a
32-member allowlist cap.

When the vault is private, Roshi computes a domain-separated leaf from the
depositor signer pubkey and verifies the proof against the vault root:

```text
sha256("roshi:vault-access:leaf:v1", depositor_pubkey)
```

Internal nodes are directionless and sorted:

```text
sha256("roshi:vault-access:node:v1", min(left, right), max(left, right))
```

When a tree level has an odd number of nodes, the lone final node is promoted to
the next level unchanged. It is not duplicated.

The `roshi-client` crate provides an `AccessMerkleTree` helper for building
roots and proofs with this convention.

## Invariants

- Admin-only instructions must verify `vault.admin`.
- Manage instructions must verify `vault.strategist`.
- NAV update instructions must verify `vault.nav_authority`.
- Withdrawal processing must verify `vault.withdrawal_authority`.
- Pause flags gate behavior, not role identity.
- Private access gates deposits, not redemptions or withdrawal settlement.
- Proofs are supplied per deposit and are never stored on-chain.
- The proof leaf is derived from the depositor signer, not from a token account.
