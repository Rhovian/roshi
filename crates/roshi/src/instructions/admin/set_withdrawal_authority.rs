use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_vault_as_admin, SetWithdrawalAuthorityArgs};

/// Implements [`crate::instructions::RoshiInstructionTag::SetWithdrawalAuthority`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account whose withdrawal authority is updated.
pub fn try_set_withdrawal_authority(
    accounts: &[AccountInfo],
    args: SetWithdrawalAuthorityArgs,
) -> ProgramResult {
    update_writable_vault_as_admin(accounts, |vault| {
        vault.withdrawal_authority = args.withdrawal_authority;
        Ok(())
    })
}
