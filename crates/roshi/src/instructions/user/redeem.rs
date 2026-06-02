use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use crate::{
    instructions::{accounts::RedeemContext, token, RedeemArgs},
    state::withdrawal_ticket::WithdrawalTicket,
};
use roshi_interface::{error::RoshiError, math::assets_for_redeem};

/// Implements [`crate::instructions::RoshiInstructionTag::Redeem`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Share owner (redeemer).
/// 1. `[writable]` Vault account receiving the redeem accounting update.
/// 2. `[writable]` Owner share token account (burn source).
/// 3. `[writable]` Share mint (`vault.share_mint`).
/// 4. `[]` Recipient base token account (payout destination).
/// 5. `[writable]` Withdrawal ticket PDA (created here).
/// 6. `[]` System program.
/// 7. `[]` SPL Token program.
///
/// Redemptions are asynchronous: vault assets are deployed off-chain, so a
/// redeem locks in the current share price, burns the shares immediately (so
/// they cannot be redeemed twice or transferred while a claim is outstanding),
/// and records a withdrawal ticket. The owed base assets are paid out later by
/// `process_withdrawals`.
///
/// Rejects redemptions while withdrawals are paused, computes the owed base
/// assets at the current price, enforces `min_assets_out`, burns the shares,
/// carves the owed assets out of `total_assets` into `pending_withdrawal_assets`
/// (keeping the share price intact for remaining holders), and reduces
/// `total_shares`.
pub fn try_redeem(accounts: &[AccountInfo], args: RedeemArgs) -> ProgramResult {
    let context = RedeemContext::load(accounts, &args)?;
    let vault = &context.vault;

    if vault.withdrawals_paused()? {
        return Err(RoshiError::VaultPaused.into());
    }

    let assets_owed = assets_for_redeem(args.shares, vault.total_assets, vault.total_shares)?;
    if assets_owed < args.min_assets_out {
        return Err(RoshiError::SlippageExceeded.into());
    }

    // Burn the shares up front; the signer authorizes the burn as token-account
    // owner or delegate.
    token::burn(
        context.token_program,
        context.share_source,
        context.share_mint,
        context.owner,
        args.shares,
    )?;

    let ticket = WithdrawalTicket::new(
        context.vault_account.key.to_bytes(),
        context.owner.key.to_bytes(),
        context.recipient_token_account.key.to_bytes(),
        args.ticket_index,
        args.shares,
        assets_owed,
        context.ticket_bump(),
    );
    context.create_ticket(ticket)?;

    context.store(|vault| {
        vault.total_shares = vault
            .total_shares
            .checked_sub(args.shares)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        vault.total_assets = vault
            .total_assets
            .checked_sub(assets_owed)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        vault.pending_withdrawal_assets = vault
            .pending_withdrawal_assets
            .checked_add(assets_owed)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        Ok(())
    })
}
