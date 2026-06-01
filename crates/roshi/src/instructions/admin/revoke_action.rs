use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::RevokeActionContext, RevokeActionArgs};

/// Implements [`crate::instructions::RoshiInstructionTag::RevokeAction`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Vault admin (reclaims the Action account rent).
/// 1. `[]` Vault account that scopes the action.
/// 2. `[writable]` Action PDA derived from `(vault, action_hash)`.
///
/// Verifies the vault admin, checks the Action PDA seeds and vault scope, and
/// closes the authorized action so the CPI pattern can no longer be executed.
pub fn try_revoke_action(accounts: &[AccountInfo], args: RevokeActionArgs) -> ProgramResult {
    let context = RevokeActionContext::load(accounts, &args.action_hash)?;
    context.close()
}
