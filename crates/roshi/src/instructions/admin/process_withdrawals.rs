use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use crate::{
    instructions::{
        accounts::{close_account, ProcessWithdrawalsContext},
        token, ProcessWithdrawalsArgs,
    },
    state::{sub_account::VaultSubAccount, vault::Vault, withdrawal_ticket::WithdrawalTicket},
};
use roshi_interface::error::RoshiError;
use roshi_interface::math::assets_for_shares;

/// Implements [`crate::instructions::RoshiInstruction::ProcessWithdrawals`].
///
/// # Accounts
///
/// 0. `[signer]` Vault withdrawal authority.
/// 1. `[writable]` Vault account containing withdrawal queue state.
/// 2. `[]` Withdraw subaccount PDA (`vault.withdraw_sub_account`).
/// 3. `[writable]` Withdraw subaccount base custody token account.
/// 4. `[]` Share mint (`vault.share_mint`).
/// 5. `[]` SPL Token program.
/// 6. `..` Repeated `[writable]` withdrawal ticket, `[writable]` owner, and
///    `[writable]` destination token account groups.
///
/// Verifies the withdrawal authority, validates each supplied ticket, transfers
/// owed base assets from withdraw-subaccount custody to the recorded recipient,
/// closes settled tickets back to their owners, and reduces pending assets.
/// Unpriced tickets are struck first if enough NAV report epochs have elapsed;
/// a ticket whose entitlement floors to zero settles as a zero payout and is
/// closed like any other, so dust can never wedge the queue.
pub fn try_process_withdrawals(
    accounts: &[AccountInfo],
    _args: ProcessWithdrawalsArgs,
) -> ProgramResult {
    let context = ProcessWithdrawalsContext::load(accounts)?;
    process(context)
}

fn process(mut context: ProcessWithdrawalsContext) -> ProgramResult {
    let share_supply = token::mint_supply(context.share_mint)?;
    let mut settled_assets = 0u64;
    for settlement in &mut context.tickets {
        if settlement.ticket.assets_owed == 0 {
            strike_ticket(&mut context.vault, share_supply, &mut settlement.ticket)?;
        }
        settled_assets = settled_assets
            .checked_add(settlement.ticket.assets_owed)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
    }

    if settled_assets > context.vault.pending_withdrawal_assets {
        return Err(RoshiError::InvalidVaultState.into());
    }

    let sub_account_bump = [context.sub_account_bump];
    let withdraw_sub_account = [context.vault.withdraw_sub_account];
    let signer_seeds: &[&[u8]] = &[
        VaultSubAccount::SEED,
        context.vault_account.key.as_ref(),
        &withdraw_sub_account,
        &sub_account_bump,
    ];

    for settlement in &context.tickets {
        token::transfer_signed(
            context.token_program,
            context.custody,
            settlement.destination,
            context.sub_account,
            settlement.ticket.assets_owed,
            signer_seeds,
        )?;
        close_account(settlement.ticket_account, settlement.owner)?;
    }

    context.vault.pending_withdrawal_assets = context
        .vault
        .pending_withdrawal_assets
        .checked_sub(settled_assets)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;

    context.store_vault()
}

fn strike_ticket(
    vault: &mut Vault,
    active_share_supply: u64,
    ticket: &mut WithdrawalTicket,
) -> ProgramResult {
    let earliest_epoch = ticket
        .request_epoch
        .checked_add(crate::state::withdrawal_ticket::WITHDRAWAL_STRIKE_DELAY_EPOCHS)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    if vault.report_epoch < earliest_epoch || ticket.request_epoch > vault.report_epoch {
        return Err(RoshiError::InvalidWithdrawalTicketAccount.into());
    }

    // Zero is a valid strike: a dust ticket whose entitlement floors to
    // nothing settles as a zero payout and closes in this same call, instead
    // of wedging forever (it cannot be cancelled once strike-eligible).
    let economic_share_supply = vault.economic_share_supply(active_share_supply)?;
    let assets_owed = assets_for_shares(
        ticket.shares_burned,
        vault.total_assets,
        economic_share_supply,
        vault.base_decimals,
    )?;

    vault.requested_withdrawal_shares = vault
        .requested_withdrawal_shares
        .checked_sub(ticket.shares_burned)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    vault.total_assets = vault
        .total_assets
        .checked_sub(assets_owed)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    vault.pending_withdrawal_assets = vault
        .pending_withdrawal_assets
        .checked_add(assets_owed)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    ticket.assets_owed = assets_owed;

    Ok(())
}
