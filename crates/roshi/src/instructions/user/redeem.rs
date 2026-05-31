use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::RedeemArgs;

/// Implements [`crate::instructions::RoshiInstructionTag::Redeem`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Share owner.
/// 1. `[writable]` Vault account receiving the redeem accounting update.
/// 2. `[writable]` User share account or share accounting source.
/// 3. `[writable]` Withdraw custody account for immediate base-asset payment.
/// 4. `[writable]` User base-asset destination account.
/// 5. `[writable]` Optional withdrawal ticket PDA for queued redemptions.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation rejects
/// redemptions while withdrawals are paused, computes assets owed from the
/// current share price, enforces `min_assets_out`, burns or accounts shares,
/// reduces `total_shares` and `total_assets`, and either pays immediately from
/// `vault.withdraw_sub_account` custody or writes a queued withdrawal ticket.
pub fn try_redeem(_accounts: &[AccountInfo], _args: RedeemArgs) -> ProgramResult {
    Ok(())
}
