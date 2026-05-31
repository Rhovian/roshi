use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{admin::vault_update::update_vault_as_admin, SetStrategistArgs};

/// Implements [`crate::instructions::RoshiInstruction::SetStrategist`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account whose strategist is updated.
pub fn try_set_strategist(accounts: &[AccountInfo], args: SetStrategistArgs) -> ProgramResult {
    update_vault_as_admin(accounts, |vault| {
        vault.strategist = args.strategist;
        Ok(())
    })
}
