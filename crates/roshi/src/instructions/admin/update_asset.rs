use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::UpdateAssetArgs;

/// Implements [`crate::instructions::RoshiInstruction::UpdateAsset`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[]` Vault account that owns the asset config.
/// 2. `[writable]` Asset PDA for the supported non-base asset.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// vault admin, loads the supported non-base Asset PDA, validates replacement
/// custody and oracle fields, and atomically replaces mutable asset settings.
pub fn try_update_asset(_accounts: &[AccountInfo], _args: UpdateAssetArgs) -> ProgramResult {
    Ok(())
}
