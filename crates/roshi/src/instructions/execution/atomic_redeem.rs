use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use super::shared::{invoke_authorized_cpi, validate_authorized_cpi};
use crate::{
    instructions::{
        accounts::{AtomicRedeemContext, ValidatedManageAccounts},
        token, AtomicRedeemArgs,
    },
    state::{
        action::{Action, ActionScope},
        sub_account::VaultSubAccount,
    },
};
use roshi_interface::{error::RoshiError, math::assets_for_redeem};

/// Implements [`crate::instructions::RoshiInstruction::AtomicRedeem`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Owner (redeeming user; authorizes share burn).
/// 1. `[writable]` Vault.
/// 2. `[writable]` User share token account (burn source).
/// 3. `[writable]` Share mint (`vault.share_mint`).
/// 4. `[writable]` Recipient base token account (payout destination).
/// 5. `[writable]` Vault base custody token account.
/// 6. `[]` Base SPL Token program.
/// 7. `[]` Subaccount PDA derived from `(vault, sub_account)`.
/// 8. `[]` Action PDA derived from `(vault, recomputed_action_hash)`.
/// 9. `[]` Classic SPL Token program.
/// 10. `..` CPI account section. `accounts_start` is relative to this section,
///     and the target CPI program account must follow the selected CPI metas.
///
/// Atomically unwinds one pre-authorized vault position CPI, bounds the CPI
/// amount by the caller's share entitlement, pays out realized base proceeds,
/// burns the caller's shares, and decreases vault total assets by the payout.
pub fn try_atomic_redeem(accounts: &[AccountInfo], args: AtomicRedeemArgs) -> ProgramResult {
    let mut context = AtomicRedeemContext::load(accounts, &args)?;

    if context.vault.withdrawals_paused()? {
        return Err(RoshiError::VaultPaused.into());
    }

    if args.shares == 0 {
        return Err(RoshiError::ZeroOutput.into());
    }

    if context.action.scope != ActionScope::AtomicRedeem {
        return Err(RoshiError::UnauthorizedAction.into());
    }

    let max_assets_owed = validate_redeem_entitlement(&context, args.shares, &args.ix_data)?;

    let received = execute_unwind_cpi(
        context.cpi_accounts,
        &context.action,
        *context.vault_account.key,
        *context.sub_account.key,
        args.sub_account,
        context.sub_account_bump,
        context.user_share_account.key,
        context.custody,
        args.program_id,
        args.accounts_start,
        args.accounts_len,
        args.account_flags,
        args.ix_data,
    )?;
    if received > max_assets_owed {
        return Err(RoshiError::WithdrawalExceedsEntitlement.into());
    }
    if received < args.min_output {
        return Err(RoshiError::SlippageExceeded.into());
    }

    settle_atomic_redeem(&mut context, args.sub_account, args.shares, received)
}

#[inline(never)]
fn validate_redeem_entitlement(
    context: &AtomicRedeemContext,
    shares: u64,
    ix_data: &[u8],
) -> Result<u64, ProgramError> {
    let share_balance = token::token_amount(context.user_share_account)?;
    if share_balance < shares {
        return Err(RoshiError::InsufficientShares.into());
    }

    let share_supply = token::mint_supply(context.share_mint)?;
    let economic_share_supply = context.vault.economic_share_supply(share_supply)?;
    let max_assets_owed = assets_for_redeem(
        shares,
        context.vault.total_assets,
        economic_share_supply,
        context.vault.base_decimals,
    )
    .map_err(ProgramError::from)?;

    let withdrawal_amount = decode_withdrawal_amount(ix_data, &context.action)?;
    if withdrawal_amount > max_assets_owed {
        return Err(RoshiError::WithdrawalExceedsEntitlement.into());
    }

    Ok(max_assets_owed)
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn execute_unwind_cpi<'info>(
    cpi_accounts: &[AccountInfo<'info>],
    action: &Action,
    vault_key: Pubkey,
    sub_account_key: Pubkey,
    sub_account_index: u8,
    sub_account_bump: u8,
    user_share_account_key: &Pubkey,
    custody: &AccountInfo<'info>,
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    account_flags: Vec<roshi_interface::instructions::AccountFlags>,
    ix_data: Vec<u8>,
) -> Result<u64, ProgramError> {
    let validated_accounts = ValidatedManageAccounts {
        action: *action,
        vault_key,
        sub_account_key,
        sub_account_index,
        sub_account_bump,
    };

    let authorized_cpi = validate_authorized_cpi(
        cpi_accounts,
        &validated_accounts,
        program_id,
        accounts_start,
        accounts_len,
        account_flags,
        ix_data,
    )?;
    if authorized_cpi.has_account_meta(user_share_account_key) {
        return Err(RoshiError::UnauthorizedAction.into());
    }

    let pre = token::token_amount(custody)?;
    invoke_authorized_cpi(&authorized_cpi)?;
    token::verify_custody_account(custody, &sub_account_key)?;
    let post = token::token_amount(custody)?;

    post.checked_sub(pre)
        .ok_or(ProgramError::from(RoshiError::Overflow))
}

#[inline(never)]
fn settle_atomic_redeem(
    context: &mut AtomicRedeemContext,
    sub_account_index: u8,
    shares: u64,
    received: u64,
) -> ProgramResult {
    let sub_account_index_seed = [sub_account_index];
    let sub_account_bump_seed = [context.sub_account_bump];
    let signer_seeds: &[&[u8]] = &[
        VaultSubAccount::SEED,
        context.vault_account.key.as_ref(),
        &sub_account_index_seed,
        &sub_account_bump_seed,
    ];
    token::transfer_signed(
        context.base_token_program,
        context.custody,
        context.recipient_token_account,
        context.sub_account,
        received,
        signer_seeds,
    )?;
    token::burn(
        context.token_program,
        context.user_share_account,
        context.share_mint,
        context.owner,
        shares,
    )?;

    context.vault.total_assets = context
        .vault
        .total_assets
        .checked_sub(received)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    context.store_vault()
}

fn decode_withdrawal_amount(ix_data: &[u8], action: &Action) -> Result<u64, ProgramError> {
    let start = usize::from(action.redeem_amount_offset);
    let end = start
        .checked_add(8)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let bytes = ix_data
        .get(start..end)
        .ok_or(ProgramError::from(RoshiError::InstructionSliceOutOfBounds))?;

    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}
