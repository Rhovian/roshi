# Subaccounts

Roshi vaults use subaccount PDAs as signer authorities for custody and strategy
execution.

## PDA

Each vault has up to 256 subaccount signer addresses:

```text
[b"sub_account", vault, sub_account_index]
```

where `sub_account_index` is a `u8`.

Subaccounts are not Roshi-owned data accounts. They are PDA authorities. A
subaccount can:

- own SPL token accounts,
- act as a signer in authorized CPIs.

## Vault Defaults

The vault stores default subaccounts for user-facing flows:

```rust
deposit_sub_account: u8,
withdraw_sub_account: u8,
```

Deposits should route custody into the deposit subaccount by default.

Queued withdrawal settlement should pay from the withdraw subaccount by default.
Open withdrawal tickets remain vault-scoped wallet liabilities; rotating the
default withdraw subaccount only changes the default payment source.

Strategy execution is explicit: every `manage` or `manage_batch` action selects
the subaccount that signs that CPI.

## Manage

Single manage layout:

```text
[strategist, vault, subaccount PDA, Action PDA, CPI accounts...]
```

Batch manage layout:

```text
[strategist, vault, subaccount PDA 0, Action PDA 0, subaccount PDA 1, Action PDA 1, ..., CPI accounts...]
```

If a CPI needs the vault authority to sign, include the selected subaccount PDA
inside that CPI account slice. Roshi promotes the matching CPI meta to signer
and calls `invoke_signed` with the subaccount seeds.

## Why Subaccounts

Subaccounts let a vault isolate custody and permissions without deploying a new
vault:

- separate deposit and withdrawal liquidity,
- isolate strategy positions by venue or risk bucket,
- expose a narrow signer to downstream protocols,
- move assets between subaccounts through authorized CPIs,
- reduce the blast radius of a bad action authorization.

## Invariants

- A subaccount is always scoped to one vault.
- A CPI can only receive a subaccount signer for the subaccount selected in the
  instruction args.
- Action hashes include the effective signer/writable flags supplied to the CPI.
- `manage_paused` blocks strategy execution across all subaccounts.
