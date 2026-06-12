use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use crate::instructions::{accounts::update_writable_vault_as_admin, WriteDownFeesArgs};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::WriteDownFees`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account.
///
/// Forgives accrued fee liability without moving tokens: `fees_payable`
/// shrinks by `args.amount`, gross NAV is untouched (`total_assets` is
/// recomputed at the next report from unchanged gross and the smaller
/// liabilities). This unwedges `report_nav` when losses ate into the fee
/// cushion (`gross < fees_payable + pending_withdrawal_assets`). Struck
/// withdrawal tickets remain inviolable — losses deeper than the fee cushion
/// leave the vault wedged by design.
pub fn try_write_down_fees(accounts: &[AccountInfo], args: WriteDownFeesArgs) -> ProgramResult {
    if args.amount == 0 {
        return Err(RoshiError::InvalidWriteDownAmount.into());
    }

    update_writable_vault_as_admin(accounts, |vault| {
        vault.fees_payable = vault
            .fees_payable
            .checked_sub(args.amount)
            .ok_or(ProgramError::from(RoshiError::InvalidWriteDownAmount))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_amount() {
        assert_eq!(
            try_write_down_fees(&[], WriteDownFeesArgs { amount: 0 }),
            Err(ProgramError::from(RoshiError::InvalidWriteDownAmount))
        );
    }
}
