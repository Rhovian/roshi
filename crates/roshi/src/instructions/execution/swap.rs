use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_sysvar::{clock::Clock, Sysvar};

use super::shared::{invoke_authorized_cpi, validate_authorized_cpi};
use crate::instructions::{
    accounts::{SwapContext, ValidatedManageAccounts},
    token, SwapArgs,
};
use roshi_interface::{
    error::RoshiError,
    math::{mul_div_ceil_u64, BPS_DENOMINATOR},
};

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
/// 6. `..` Valuation accounts (only when `max_swap_slippage_bps > 0`; see
///    [`SwapContext`]).
/// 7. `..` CPI account section. `accounts_start` is relative to this section,
///    and the target CPI program account must follow the selected CPI metas.
///
/// Executes one pre-authorized swap CPI, enforces realized input/output
/// balance bounds on the fixed custody accounts, and — when the vault's swap
/// slippage bound is configured — values both endpoints through the deposit
/// pricing path and requires
/// `received_value >= spent_value * (1 - max_swap_slippage_bps)`.
// The context carries a full `Vault` plus an `Action` by value; give the
// handler its own stack frame instead of growing the entrypoint's.
#[inline(never)]
pub fn try_swap<'info>(accounts: &'info [AccountInfo<'info>], args: SwapArgs) -> ProgramResult {
    let context = SwapContext::load(accounts, &args)?;

    let (spent, received) = execute_swap_cpi(&context, args)?;

    verify_swap_value(&context, spent, received)
}

/// Run the pre-authorized CPI and return the realized `(spent, received)`
/// custody deltas, enforcing the caller's amount bounds.
#[inline(never)]
fn execute_swap_cpi<'a, 'info>(
    context: &SwapContext<'a, 'info>,
    args: SwapArgs,
) -> Result<(u64, u64), ProgramError>
where
    'a: 'info,
{
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

    // The CPI is signed by the sub-account PDA, so the route can move any
    // sub-account custody it was handed — not just the two named endpoints the
    // value bound measures. Snapshot every other writable custody and require it
    // untouched, so a route can't drain a sibling custody past a flat bound.
    let custody = authorized_cpi
        .snapshot_writable_custody(&[*context.input_custody.key, *context.output_custody.key])?;

    invoke_authorized_cpi(&authorized_cpi)?;

    authorized_cpi.verify_custody_unchanged(&custody)?;

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

    Ok((spent, received))
}

/// Oracle-valued slippage bound: the realized output must be worth at least
/// the realized input minus the configured tolerance, both valued in base
/// atoms through the same pricing path deposits use. The caller-supplied
/// `max_in`/`min_out` protect amounts; this bounds *value*, so a compromised
/// swap authority cannot leak NAV through an unfavorable authorized route.
#[inline(never)]
fn verify_swap_value<'a, 'info>(
    context: &SwapContext<'a, 'info>,
    spent: u64,
    received: u64,
) -> ProgramResult
where
    'a: 'info,
{
    let Some(valuation) = &context.valuation else {
        return Ok(());
    };

    let clock = Clock::get()?;
    let (spent_value, received_value) =
        valuation.values(&context.vault, spent, received, &clock)?;

    let tolerated_bps = BPS_DENOMINATOR
        .checked_sub(context.vault.controls.max_swap_slippage_bps)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    let min_value = mul_div_ceil_u64(
        spent_value,
        u64::from(tolerated_bps),
        u64::from(BPS_DENOMINATOR),
    )?;
    if received_value < min_value {
        return Err(RoshiError::SlippageExceeded.into());
    }

    Ok(())
}
