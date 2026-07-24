use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_vault_as_admin, SetSwapAuthorityArgs};

/// Implements [`crate::instructions::RoshiInstruction::SetSwapAuthority`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account whose swap authority is updated.
pub fn try_set_swap_authority(
    accounts: &[AccountInfo],
    args: SetSwapAuthorityArgs,
) -> ProgramResult {
    update_writable_vault_as_admin(accounts, |vault| {
        vault.swap_authority = args.swap_authority;
        Ok(())
    })
}
