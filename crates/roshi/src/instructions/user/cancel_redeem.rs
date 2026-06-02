use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_sysvar::{clock::Clock, Sysvar};

use crate::{
    instructions::{accounts::CancelRedeemContext, token, CancelRedeemArgs},
    state::{
        vault::Vault,
        withdrawal_ticket::{REDEEM_CANCEL_DELAY_SLOTS, WITHDRAWAL_STRIKE_DELAY_EPOCHS},
    },
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::CancelRedeem`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Original share owner.
/// 1. `[writable]` Vault.
/// 2. `[writable]` Open withdrawal ticket PDA.
/// 3. `[writable]` Share mint (`vault.share_mint`).
/// 4. `[writable]` Owner share token account receiving reentry shares.
/// 5. `[]` SPL Token program.
///
/// Cancelling an unstruck withdrawal ticket is a liveness escape for a missing
/// NAV report. It restores the originally burned shares after a slot delay, but
/// only before the ticket becomes eligible to be struck.
pub fn try_cancel_redeem(accounts: &[AccountInfo], args: CancelRedeemArgs) -> ProgramResult {
    let context = CancelRedeemContext::load(accounts)?;
    let vault = &context.vault;
    let ticket = context.ticket;

    if ticket.assets_owed != 0 {
        return Err(RoshiError::InvalidWithdrawalTicketAccount.into());
    }

    let earliest_strike_epoch = ticket
        .request_epoch
        .checked_add(WITHDRAWAL_STRIKE_DELAY_EPOCHS)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    let clock = Clock::get()?;
    let cancel_slot = ticket
        .request_slot
        .checked_add(REDEEM_CANCEL_DELAY_SLOTS)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    if vault.report_epoch >= earliest_strike_epoch || clock.slot < cancel_slot {
        return Err(RoshiError::InvalidWithdrawalTicketAccount.into());
    }

    if ticket.shares_burned < args.min_shares_out {
        return Err(RoshiError::SlippageExceeded.into());
    }

    let tag = vault.tag_seed()?;
    let base_mint = vault.base_mint;
    let bump = [vault.bump];
    let signer_seeds: &[&[u8]] = &[Vault::SEED, tag, &base_mint, &bump];
    token::mint_to_signed(
        context.token_program,
        context.share_mint,
        context.share_dest,
        context.vault_account,
        ticket.shares_burned,
        signer_seeds,
    )?;

    context.close_ticket()?;
    context.store_vault(|vault| {
        vault.requested_withdrawal_shares = vault
            .requested_withdrawal_shares
            .checked_sub(ticket.shares_burned)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        Ok(())
    })
}
