use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_vault_as_admin, SetPauseFlagsArgs};

/// Implements [`crate::instructions::RoshiInstruction::SetPauseFlags`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account whose pause flags are updated.
///
/// Verifies the vault admin and atomically updates the deposit, withdrawal, and
/// manage pause flags without touching the rest of the vault configuration.
pub fn try_set_pause_flags(accounts: &[AccountInfo], args: SetPauseFlagsArgs) -> ProgramResult {
    update_writable_vault_as_admin(accounts, |vault| {
        vault.set_deposits_paused(args.deposits_paused);
        vault.set_withdrawals_paused(args.withdrawals_paused);
        vault.set_manage_paused(args.manage_paused);
        Ok(())
    })
}
