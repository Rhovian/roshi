# Vault Access

Roshi vaults can be public or private.

Public vaults accept deposits from any valid depositor, subject to pause flags,
asset configuration, limits, oracle checks, and share math.

Private vaults require depositors to prove membership in an admin-controlled
Merkle allowlist of pubkeys. Access control gates new deposits only. It does
not prevent existing share owners from redeeming or claiming queued withdrawals.

## State

The vault stores:

```rust
private: bool,
access_merkle_root: [u8; 32],
```

`private = false` means the Merkle root is ignored.

`private = true` means deposits must include a valid proof for the depositor
signer against `access_merkle_root`. If `private = true` and
`access_merkle_root = [0; 32]`, the vault is effectively closed to new
deposits.

## Initialization

`InitializeVaultArgs` includes:

```rust
private: bool,
access_merkle_root: [u8; 32],
```

A vault can start public by setting `private = false`. A vault can start
private by setting `private = true` and storing the initial allowlist root.

## Updates

Access is updated through a dedicated admin instruction:

```rust
SetVaultAccess {
    private,
    access_merkle_root,
}
```

This lets the admin:

- flip a vault from public to private,
- flip a vault from private to public,
- rotate the allowlist root,
- disable new private deposits by setting `access_merkle_root = [0; 32]` while
  private mode is enabled.

Access updates should not touch roles, fees, guardrails, subaccounts, pause
flags, assets, NAV, shares, or withdrawal state.

## Proofs

Deposits carry:

```rust
access_proof: Vec<[u8; 32]>
```

Proofs are capped at 32 sibling hashes.

This is a proof-depth cap, not a 32-member allowlist cap. A balanced tree with
1,024 members has proof length 10; a tree with about one million members has
proof length 20. A proof length cap of 32 allows up to roughly `2^32` leaves,
well beyond practical transaction-size limits.

When the vault is public, the proof is ignored.

When the vault is private, the program computes a domain-separated leaf from
the depositor signer pubkey and verifies the proof against the vault root.

The leaf is:

```text
sha256("roshi:vault-access:leaf:v1", depositor_pubkey)
```

Each internal node is directionless and sorted:

```text
sha256("roshi:vault-access:node:v1", min(left, right), max(left, right))
```

This avoids storing side bits on-chain. Clients and off-chain allowlist tooling
must use the same construction.

When a tree level has an odd number of nodes, the lone final node is promoted to
the next level unchanged. It is not duplicated.

The `roshi-client` crate provides an `AccessMerkleTree` helper for building
roots and proofs with this convention.

## Invariants

- Private access gates deposits only.
- Redemptions and withdrawal processing must not require allowlist membership.
- `SetVaultAccess` must be admin-only.
- Proofs are supplied per deposit and are never stored on-chain.
- Proofs longer than 32 sibling hashes must be rejected.
- The proof leaf is derived from the depositor signer, not from a token account.
- The program should use the shared interface proof verifier so clients and
  on-chain code agree on the root construction.
