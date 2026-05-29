use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::InitializeAssetArgs;

pub fn try_initialize_asset(
    _accounts: &[AccountInfo],
    _args: InitializeAssetArgs,
) -> ProgramResult {
    // TODO: verify vault admin, reject the vault base mint, derive the Asset
    // PDA from (vault, asset_mint), validate custody/oracle accounts, and write
    // the supported non-base asset config. The oracle must report this asset
    // directly in vault base units.
    Ok(())
}
