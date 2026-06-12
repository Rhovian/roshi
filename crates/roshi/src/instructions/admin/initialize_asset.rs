use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::{
    instructions::{accounts::InitializeAssetContext, InitializeAssetArgs},
    state::asset::Asset,
};

/// Implements [`crate::instructions::RoshiInstruction::InitializeAsset`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Vault admin (funds the Asset account).
/// 1. `[]` Vault account that owns the asset config.
/// 2. `[]` Asset mint.
/// 3. `[writable]` Asset PDA derived from `(vault, asset_mint)`.
/// 4. `[]` System program for Asset account creation.
///
/// Verifies the vault admin, rejects the vault base mint, and writes the
/// supported non-base asset config. The oracle is recorded as configuration and
/// must quote one whole asset token — in whole base tokens (direct), or in the
/// quote currency shared with the vault's `base_oracle` (routed); custody is
/// the `ATA(deposit_sub_account, asset_mint)`, derived (not stored) and
/// verified at deposit time.
pub fn try_initialize_asset(accounts: &[AccountInfo], args: InitializeAssetArgs) -> ProgramResult {
    let context = InitializeAssetContext::load(accounts, &args)?;
    let asset = Asset::new(
        context.vault_key().to_bytes(),
        args.asset_mint,
        args.oracle,
        args.asset_decimals,
        args.enabled,
        args.routed,
        args.deposit_cap_atoms,
        context.asset_bump(),
    )?;
    context.create_and_store(asset)
}
