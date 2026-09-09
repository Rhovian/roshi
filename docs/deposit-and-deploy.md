# Deposit and deploy

`DepositAndDeploy` (instruction tag 35) deposits base, prices and mints shares
exactly like `Deposit`, then relays a single admin-authorized `Deploy` action
(scope tag 4). The depositor signs; no strategist signature is needed. Deposit
pause, private-vault proof, NAV pricing, deposit cap, and `min_shares_out` all
apply. A failed relay rolls back the deposit and share mint.

The first eight accounts match a base `Deposit`; next come the sub-account PDA
for `vault.deposit_sub_account` and the Action PDA, followed by the CPI section.
`accounts_start` is relative to that section, and the executable CPI program
must immediately follow its selected metas. The instruction derives the base
mint and sub-account index from the vault. Non-base deposits need a conversion
leg and are not supported here.

Author a route by ingesting **every** CPI meta, including readonly accounts.
The hash pins account keys and effective signer/writable flags, including the
sub-account's promoted signer flag. The decoded little-endian `u64` at the
admin-configured `amount_offset` must equal the deposited `amount`. For a klend
`deposit_reserve_liquidity` route, authoring looks like this (use the ordered
metas from the venue IDL, with the sub-account marked as a signer):

```rust,ignore
let ops = Ops::new(
    (0..u8::try_from(klend_metas.len())?)
        .map(|index| Op::IngestAccount { index })
        .chain([
            Op::IngestInstruction { offset: 0, len: 8 },
            Op::IngestInstructionDataSize,
        ]),
)?;
let action_hash = compute_action_hash_from_metas(
    &klend_program, &ops, &klend_metas, &supply_data, &[],
)?;
// Authorize with scope = ActionScope::Deploy, amount_offset = 8.
// supply_data is exactly 16 bytes: Anchor discriminator + liquidity_amount.
```

Pin the payload size because Anchor accepts trailing bytes. Pin the reserve,
liquidity destination, and vault-owned collateral destination along with every
other meta. The amount is the route's only intended variable. `MAX_ACTION_OPS`
is 32; one op per meta plus discriminator and size fit the reserve supply route.

The relay does not revalue the vault or modify `total_assets`; only the deposit
credits it. Custody accounts must remain clean after CPI, without delegates or
changed authority. Deploy actions are rejected by `manage` and `manage_batch`.
Pre-existing idle base remains deployable through strategist-authorized Manager
actions, so a public caller cannot repeatedly redeploy withdrawal liquidity.

`Action.amount_offset` replaces the name `redeem_amount_offset` without changing
serialized bytes or account layout. Existing actions and hashes remain valid.
