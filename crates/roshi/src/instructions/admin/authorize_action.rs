use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::AuthorizeActionContext, AuthorizeActionArgs};

/// Implements [`crate::instructions::RoshiInstructionTag::AuthorizeAction`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Vault admin (funds the Action account).
/// 1. `[]` Vault account that scopes the action.
/// 2. `[writable]` Action PDA derived from `(vault, action_hash)`.
/// 3. `[]` System program for Action account creation.
///
/// Verifies the vault admin, creates the Action PDA, and stores the vault,
/// approved `action_hash`, `ops`, and PDA bump used later by manage to
/// re-derive and match the authorized CPI.
pub fn try_authorize_action(accounts: &[AccountInfo], args: AuthorizeActionArgs) -> ProgramResult {
    let context = AuthorizeActionContext::load(accounts, &args.action_hash)?;
    context.create_and_store(args.action_hash, args.ops)
}
