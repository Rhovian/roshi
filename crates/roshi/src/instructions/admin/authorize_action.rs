use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::state::action::Ops;

/// Implements [`crate::instructions::RoshiInstruction::AuthorizeAction`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[]` Vault account that scopes the action.
/// 2. `[writable]` Action PDA derived from `(vault, action_hash)`.
/// 3. `[]` System program for Action account creation.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// vault admin, creates the Action PDA, and stores the vault, approved
/// `action_hash`, `ops`, and PDA bump used later by manage instructions.
pub fn try_authorize_action(
    _accounts: &[AccountInfo],
    _action_hash: [u8; 32],
    _ops: Ops,
) -> ProgramResult {
    let _ = _ops;
    Ok(())
}
