use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_vault_as_admin, SetNavAuthorityArgs};

/// Implements [`crate::instructions::RoshiInstruction::SetNavAuthority`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account whose NAV authority is updated.
pub fn try_set_nav_authority(accounts: &[AccountInfo], args: SetNavAuthorityArgs) -> ProgramResult {
    update_writable_vault_as_admin(accounts, |vault| {
        vault.nav_authority = args.nav_authority;
        Ok(())
    })
}
