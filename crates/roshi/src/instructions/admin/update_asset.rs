use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_asset_as_admin, UpdateAssetArgs};

/// Implements [`crate::instructions::RoshiInstruction::UpdateAsset`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[]` Vault account that owns the asset config.
/// 2. `[writable]` Asset PDA for the supported non-base asset.
///
/// Verifies the vault admin, loads the asset's PDA scoped to the vault, and
/// atomically replaces the mutable settings (oracle, enabled, routed).
/// Immutable fields (vault, asset mint, decimals) and the derived custody
/// account are preserved; the replacement is validated on store.
pub fn try_update_asset(accounts: &[AccountInfo], args: UpdateAssetArgs) -> ProgramResult {
    update_writable_asset_as_admin(accounts, |asset| {
        asset.oracle = args.oracle;
        asset.set_enabled(args.enabled);
        asset.set_routed(args.routed);
        Ok(())
    })
}
