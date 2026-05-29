use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::InitializeAssetArgs;

/// Implements [`crate::instructions::RoshiInstruction::InitializeAsset`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[]` Vault account that owns the asset config.
/// 2. `[writable]` Asset PDA derived from `(vault, asset_mint)`.
/// 3. `[]` Custody token account configured for this asset.
/// 4. `[]` Oracle account configured for base-denominated pricing.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// vault admin, rejects the vault base mint, validates custody and oracle
/// accounts, and writes the supported non-base asset config. The oracle must
/// report this asset directly in vault base units.
pub fn try_initialize_asset(
    _accounts: &[AccountInfo],
    _args: InitializeAssetArgs,
) -> ProgramResult {
    Ok(())
}
