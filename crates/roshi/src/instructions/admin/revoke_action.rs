use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

/// Implements [`crate::instructions::RoshiInstruction::RevokeAction`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[]` Vault account that scopes the action.
/// 2. `[writable]` Action PDA derived from `(vault, action_hash)`.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// vault admin, checks the Action PDA seeds and vault scope, and closes or
/// clears the authorized action so the CPI pattern can no longer be executed.
pub fn try_revoke_action(_accounts: &[AccountInfo], _action_hash: [u8; 32]) -> ProgramResult {
    Ok(())
}
