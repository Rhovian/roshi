use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_sysvar::{clock::Clock, Sysvar};

use crate::{
    instructions::{accounts::RedeemContext, token, RedeemArgs},
    state::withdrawal_ticket::WithdrawalTicket,
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::Redeem`].
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
/// redeem burns shares immediately (so they cannot be redeemed twice or
/// transferred while a claim is outstanding) and records an unpriced withdrawal
/// ticket. The burned shares remain in the vault's economic denominator until
/// the withdrawal authority strikes the ticket after the epoch delay.
///
/// Rejects redemptions while withdrawals are paused, computes the owed base
/// burns the shares, stores the deferred slippage bound on the ticket, and
/// tracks the burned-but-unstruck shares on the vault.
pub fn try_redeem(accounts: &[AccountInfo], args: RedeemArgs) -> ProgramResult {
    let context = RedeemContext::load(accounts, &args)?;
    let vault = &context.vault;

    if vault.withdrawals_paused()? {
        return Err(RoshiError::VaultPaused.into());
    }

    if args.shares == 0 {
        return Err(RoshiError::ZeroOutput.into());
    }
    let share_supply = token::mint_supply(context.share_mint)?;
    if args.shares > share_supply {
        return Err(RoshiError::InvalidVaultState.into());
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

    let clock = Clock::get()?;
    let ticket = WithdrawalTicket::new(
        context.vault_account.key.to_bytes(),
        context.owner.key.to_bytes(),
        context.recipient_token_account.key.to_bytes(),
        args.ticket_index,
        args.shares,
        0,
        vault.report_epoch,
        clock.slot,
        context.ticket_bump(),
    );
    context.create_ticket(ticket)?;

    context.store(|vault| {
        vault.requested_withdrawal_shares = vault
            .requested_withdrawal_shares
            .checked_add(args.shares)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        Ok(())
    })
}
