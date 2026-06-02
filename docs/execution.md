# Execution

Roshi's execution system lets a vault interact with arbitrary Solana programs
through pre-authorized CPI patterns. The protocol does not need a new on-chain
instruction for every downstream integration.

## Roles

Admin authorizes execution patterns by creating `Action` accounts.

Strategist executes those patterns through `manage` or `manage_batch`.

## Subaccounts

Roshi vaults use subaccount PDAs as signer authorities for custody and strategy
execution:

```text
[b"sub_account", vault, sub_account_index]
```

where `sub_account_index` is a `u8`.

Subaccounts are not Roshi-owned data accounts. They are bare PDA authorities
that can own SPL token accounts and sign authorized CPIs.

The vault stores default subaccounts for user-facing flows:

```rust
deposit_sub_account: u8,
withdraw_sub_account: u8,
```

Deposits route custody into the deposit subaccount by default. Queued
withdrawal settlement pays from the withdraw subaccount by default. Open
withdrawal tickets remain vault-scoped user liabilities; rotating the default
withdraw subaccount only changes the default payment source.

Strategy execution is explicit: every `manage` or `manage_batch` action selects
the subaccount that signs that CPI.

## Action Accounts

An `Action` is vault-scoped authorization for a CPI pattern.

```rust
Action {
    vault: Pubkey,
    action_hash: [u8; 32],
    ops: Ops,
    bump: u8,
}
```

Seeds:

```rust
[b"action", vault, action_hash]
```

The Action account stores the `Ops` used to recompute the hash at execution
time. This prevents the strategist from supplying a different authorization shape
than the admin approved.

## Ops

`Ops` define which parts of a CPI are included in the authorization hash.

```rust
enum Op {
    Noop,
    IngestInstruction { offset: u16, len: u8 },
    IngestAccount { index: u8 },
    IngestInstructionDataSize,
}
```

`Noop` contributes only its discriminant. It is useful when the admin wants a
distinct action hash without pinning additional CPI data.

`IngestInstruction` pins a byte slice of `ix_data`.

`IngestAccount` pins the full account meta of a CPI account at an index in the
CPI account slice: pubkey, signer flag, and writable flag.

`IngestInstructionDataSize` pins the length of `ix_data`.

The hash includes:

```text
program_id
op discriminants
op parameters
op ingested values
```

Op discriminants and parameters are included so two different op shapes cannot
collide just because they ingest the same bytes.

An empty `Ops` list authorizes any CPI data/account shape for the selected
program id. That is intentionally broad and should be used carefully.

## Authorizing Actions

The intended `authorize_action` flow is:

1. Admin chooses a CPI program id.
2. Admin chooses the `Ops` that define the allowed pattern.
3. Admin computes `action_hash` from `(program_id, ops, cpi_accounts, ix_data)`.
4. Admin creates the Action PDA for `(vault, action_hash)` and stores the `Ops`.

The current scaffold has the instruction surface for this flow; the account
creation handler is still intentionally stubbed.

## Manage

`manage` executes one authorized CPI.

Instruction data:

```rust
Manage {
    sub_account,
    program_id,
    accounts_start,
    accounts_len,
    ix_data,
}
```

Account layout:

```text
[strategist, vault, subaccount PDA, Action PDA, CPI account section...]
```

`accounts_start` is relative to the CPI accounts section, not the full Roshi
instruction account list.

For example, with the layout above:

```text
accounts_start = 0
```

starts at the first CPI account after the Action PDA.

`sub_account` selects the vault subaccount PDA:

```text
[b"sub_account", vault, sub_account]
```

If the downstream CPI needs the vault authority as a signer, the same subaccount
PDA should also appear inside the CPI account slice. Roshi verifies the fixed
subaccount PDA, marks matching CPI account metas as signed, and invokes with the
subaccount signer seeds.

Execution checks:

1. `strategist` must sign.
2. `strategist` must equal `vault.strategist`.
3. `vault` must be a Roshi `Vault` account owned by Roshi.
4. `vault` must match its stored PDA seeds and bump.
5. `vault.manage_paused` must be false.
6. `subaccount PDA` must match `[b"sub_account", vault, sub_account]`.
7. `subaccount PDA` must be a non-executable system account with no data.
8. `Action PDA` must be a Roshi `Action` account owned by Roshi.
9. `action.vault` must equal the supplied vault.
10. The Action PDA address and stored bump must match
   `[b"action", vault, action_hash]`.
11. Roshi recomputes the hash from the supplied CPI program id, stored `Ops`,
   selected CPI account slice, and `ix_data`.
12. `action.action_hash` must equal the recomputed hash.
13. The supplied target CPI program account must match `program_id`.
14. The supplied target CPI program account must be executable.
15. Roshi invokes the CPI with subaccount signer seeds.

The CPI instruction metas are created from the selected CPI account infos. Their
`is_signer` and `is_writable` flags come from the Roshi instruction's account
metas.

The target CPI program account must be supplied immediately after the selected
CPI meta account slice. Roshi verifies it matches the requested program id and
is executable, then passes it to `invoke` as an account info without including
it as an instruction meta.

## Manage Batch

`manage_batch` executes multiple authorized CPIs atomically.

Instruction data:

```rust
ManageBatch {
    actions: Vec<IndexedActionArgs>,
}
```

Each action specifies:

```rust
IndexedActionArgs {
    sub_account,
    program_id,
    accounts_start,
    accounts_len,
    ix_data,
}
```

Account layout:

```text
[strategist, vault, subaccount PDA 0, Action PDA 0, subaccount PDA 1, Action PDA 1, ..., CPI accounts...]
```

The CPI account section starts immediately after the subaccount/action pairs:

```rust
cpi_accounts_base = 2 + actions.len() * 2
```

For action `i`, Roshi uses account `2 + i * 2` as that action's subaccount PDA
and account `3 + i * 2` as that action's Action PDA. It then uses
`accounts_start` and `accounts_len` as offsets into the shared CPI account
section. The target CPI program account for that action must appear immediately
after its selected CPI account slice.

This lets multiple actions share the same CPI accounts by overlapping their
account slices.

Batch actions execute sequentially in the supplied order. Roshi revalidates the
vault, subaccount, Action account, and CPI authorization immediately before each
action is invoked, so writes from earlier actions are visible to later checks.
If any action fails, the whole transaction fails.

## Security Invariants

- Strategists can only execute for vaults where they are the configured
  strategist.
- Manage execution must not be paused.
- Vault and Action accounts must be Roshi-owned accounts of the expected type.
- The vault account must match its stored PDA seeds and bump.
- The selected subaccount PDA must match the supplied subaccount index.
- The selected subaccount PDA must be a non-executable system account with no
  data.
- Action accounts are scoped to a single vault.
- Action PDA seeds include the vault and action hash, and the stored bump must
  match the derived bump.
- The target CPI program account must be supplied, match the requested program
  id, and be executable.
- Stored `Ops` are used during execution; the strategist cannot substitute a
  different op list.
- CPI account indices in `Ops` are evaluated against the selected CPI account
  slice, not the full Roshi instruction account list.
- Account ingestion hashes pubkey, signer flag, and writable flag.
- Account ingestion hashes the effective CPI signer flag after subaccount signer
  promotion.
- CPI instruction data slices must be in bounds.
- Batch execution revalidates each action immediately before invoking it.
- Batch execution is atomic at the transaction level.
- A subaccount is always scoped to one vault.
- A CPI can only receive a subaccount signer for the subaccount selected in the
  instruction args.

## Design Notes

`IngestAccount` intentionally hashes signer and writable flags because the target
CPI program receives the full `AccountMeta`, not just the pubkey. Hashing only
the pubkey would let a strategist preserve the authorized account key while
changing whether the CPI sees that account as writable or signed.

The execution system authorizes CPI shape; it does not make downstream programs
safe. Admins still need to understand what a target CPI does, which accounts it
can modify, and which signer authorities are being exposed.
