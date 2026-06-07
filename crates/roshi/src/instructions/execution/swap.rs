use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use super::shared::{invoke_authorized_cpi, validate_authorized_cpi};
use crate::instructions::{
    accounts::{SwapContext, ValidatedManageAccounts},
    token, SwapArgs,
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::Swap`].
///
/// # Accounts
///
/// 0. `[signer]` Swap authority (verified against `vault.swap_authority`).
/// 1. `[]` Vault.
/// 2. `[]` Subaccount PDA derived from `(vault, sub_account)`.
/// 3. `[writable]` Input custody token account (owner = subaccount PDA).
/// 4. `[writable]` Output custody token account (owner = subaccount PDA).
/// 5. `[]` Action PDA derived from `(vault, recomputed_action_hash)`.
/// 6. `..` CPI account section. `accounts_start` is relative to this section,
///    and the target CPI program account must follow the selected CPI metas.
///
/// Executes one pre-authorized swap CPI and enforces realized input/output
/// balance bounds on the fixed custody accounts.
pub fn try_swap(accounts: &[AccountInfo], args: SwapArgs) -> ProgramResult {
    let context = SwapContext::load(accounts, &args)?;

    let in_pre = token::token_amount(context.input_custody)?;
    let out_pre = token::token_amount(context.output_custody)?;

    let validated_accounts = ValidatedManageAccounts {
        action: context.action,
        vault_key: context.vault_key,
        sub_account_key: *context.sub_account.key,
        sub_account_index: context.sub_account_index,
        sub_account_bump: context.sub_account_bump,
    };
    let authorized_cpi = validate_authorized_cpi(
        context.cpi_accounts,
        &validated_accounts,
        args.program_id,
        args.accounts_start,
        args.accounts_len,
        args.account_flags,
        args.ix_data,
    )?;

    invoke_authorized_cpi(&authorized_cpi)?;

    token::verify_custody_account(context.input_custody, context.sub_account.key)?;
    token::verify_custody_account(context.output_custody, context.sub_account.key)?;

    let in_post = token::token_amount(context.input_custody)?;
    let out_post = token::token_amount(context.output_custody)?;
    let spent = in_pre
        .checked_sub(in_post)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    let received = out_post
        .checked_sub(out_pre)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;

    if spent > args.max_in {
        return Err(RoshiError::SlippageExceeded.into());
    }
    if received < args.min_out {
        return Err(RoshiError::SlippageExceeded.into());
    }

    Ok(())
}
