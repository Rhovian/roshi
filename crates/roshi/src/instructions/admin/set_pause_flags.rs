use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::SetPauseFlagsArgs;

/// Implements [`crate::instructions::RoshiInstructionTag::SetPauseFlags`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account whose pause flags are updated.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// vault admin and atomically updates the deposit, withdrawal, and manage pause
/// flags without touching the rest of the vault configuration.
pub fn try_set_pause_flags(_accounts: &[AccountInfo], _args: SetPauseFlagsArgs) -> ProgramResult {
    Ok(())
}
