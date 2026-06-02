use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::{
    instructions::{accounts::InitializeAssetContext, InitializeAssetArgs},
    state::asset::Asset,
};

/// Implements [`crate::instructions::RoshiInstructionTag::InitializeAsset`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Vault admin (funds the Asset account).
/// 1. `[]` Vault account that owns the asset config.
/// 2. `[writable]` Asset PDA derived from `(vault, asset_mint)`.
/// 3. `[]` System program for Asset account creation.
///
/// Verifies the vault admin, rejects the vault base mint, and writes the
/// supported non-base asset config. The custody token account and oracle are
/// recorded as configuration; the oracle must report this asset directly in
/// vault base atoms, and on-chain custody/oracle account validation happens at
/// deposit time. The vault's base decimals are read from the vault when needed.
pub fn try_initialize_asset(accounts: &[AccountInfo], args: InitializeAssetArgs) -> ProgramResult {
    let context = InitializeAssetContext::load(accounts, &args)?;
    let asset = Asset::new(
        context.vault_key().to_bytes(),
        args.asset_mint,
        args.custody_token_account,
        args.oracle,
        args.asset_decimals,
        args.max_price_change_bps,
        args.deposit_limit,
        args.enabled,
        context.asset_bump(),
    )?;
    context.create_and_store(asset)
}
