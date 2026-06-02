use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use crate::{
    instructions::{accounts::CancelRedeemContext, token, CancelRedeemArgs},
    state::vault::Vault,
};
use roshi_interface::{error::RoshiError, math::shares_for_deposit};

/// Implements [`crate::instructions::RoshiInstructionTag::CancelRedeem`].
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
/// Cancelling an open withdrawal ticket favors active holders by treating the
/// fixed pending claim as a fresh deposit into the active vault at current NAV.
/// The owner receives newly computed shares, not necessarily the originally
/// burned share count.
pub fn try_cancel_redeem(accounts: &[AccountInfo], args: CancelRedeemArgs) -> ProgramResult {
    let context = CancelRedeemContext::load(accounts)?;
    let vault = &context.vault;
    let ticket = context.ticket;

    if vault.pending_withdrawal_assets < ticket.assets_owed {
        return Err(RoshiError::Overflow.into());
    }

    let shares_to_mint = if vault.total_assets == 0 && vault.total_shares == 0 {
        ticket.shares_burned
    } else {
        shares_for_deposit(ticket.assets_owed, vault.total_assets, vault.total_shares)?
    };
    if shares_to_mint < args.min_shares_out {
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
        shares_to_mint,
        signer_seeds,
    )?;

    context.close_ticket()?;
    context.store_vault(|vault| {
        vault.pending_withdrawal_assets = vault
            .pending_withdrawal_assets
            .checked_sub(ticket.assets_owed)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        vault.total_assets = vault
            .total_assets
            .checked_add(ticket.assets_owed)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        vault.total_shares = vault
            .total_shares
            .checked_add(shares_to_mint)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        Ok(())
    })
}
