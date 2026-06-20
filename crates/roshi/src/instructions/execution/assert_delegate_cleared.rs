use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

/// Implements [`crate::instructions::RoshiInstruction::AssertDelegateCleared`].
///
/// # Accounts
///
/// 0. `[]` Token account that must carry no delegate and zero delegated amount.
///
/// A generic, permissionless backstop: it only reads the supplied account and
/// fails loudly if it still carries a delegate. `FlashApprove` (#21) binds this
/// as a committed sibling after the top-level `flash_repay` so a committed flash
/// fee that exceeds the lender's can never leave a residual delegate on custody.
pub fn try_assert_delegate_cleared(accounts: &[AccountInfo]) -> ProgramResult {
    let account = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    crate::instructions::token::assert_delegate_cleared(account)
}
