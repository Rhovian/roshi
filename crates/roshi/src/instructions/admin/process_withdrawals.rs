use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::ProcessWithdrawalsArgs;

/// Implements [`crate::instructions::RoshiInstructionTag::ProcessWithdrawals`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault withdrawal authority.
/// 1. `[writable]` Vault account containing withdrawal queue state.
/// 2. `[]` Withdraw subaccount PDA or its custody token account.
/// 3. `..` Repeated withdrawal ticket, owner, and destination account groups.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// withdrawal authority, validates queued tickets, transfers owed base assets
/// from withdraw-subaccount custody to each ticket owner, closes or clears
/// settled ticket slots, advances processed epochs, and reduces pending assets.
pub fn try_process_withdrawals(
    _accounts: &[AccountInfo],
    _args: ProcessWithdrawalsArgs,
) -> ProgramResult {
    Ok(())
}
