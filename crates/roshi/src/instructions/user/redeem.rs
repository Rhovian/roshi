use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_sysvar::{clock::Clock, Sysvar};

use crate::{
    instructions::{accounts::RedeemContext, token, RedeemArgs},
    state::withdrawal_ticket::WithdrawalTicket,
};
use roshi_interface::{error::RoshiError, math::assets_for_redeem};

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
/// Rejects redemptions while withdrawals are paused and redemptions whose
/// entitlement rounds to zero at the current NAV, burns the shares, and
/// tracks the burned-but-unstruck shares on the vault. The ticket PDA is
/// seeded by the owner, so each owner has their own ticket-index namespace.
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

    // Fail fast on dust: reject a redeem whose entitlement already rounds to
    // zero at the current effective NAV. Pricing happens later at strike
    // (where zero is tolerated, since NAV can move); this guard just refuses
    // to burn shares into a ticket that is worthless today.
    let clock = Clock::get()?;
    let economic_share_supply = vault.economic_share_supply(share_supply)?;
    assets_for_redeem(
        args.shares,
        vault.effective_total_assets(clock.unix_timestamp)?,
        economic_share_supply,
        vault.base_decimals,
    )?;

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
