use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::InitializeSubAccountArgs;

/// Implements [`crate::instructions::RoshiInstructionTag::InitializeSubAccount`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[]` Vault account that scopes the subaccount.
/// 2. `[]` Subaccount PDA derived from `(vault, index)`.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// vault admin and checks the subaccount PDA seeds. Subaccounts are PDA signer
/// authorities only; Roshi does not create Roshi-owned data accounts for them.
pub fn try_initialize_sub_account(
    _accounts: &[AccountInfo],
    _args: InitializeSubAccountArgs,
) -> ProgramResult {
    Ok(())
}
