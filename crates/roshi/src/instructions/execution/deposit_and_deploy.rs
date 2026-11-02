use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use super::{
    amount::decode_withdrawal_amount,
    shared::{invoke_authorized_cpi, validate_authorized_cpi},
};
use crate::{
    instructions::{
        accounts::DepositAndDeployContext, user::deposit::execute_deposit, DepositAndDeployArgs,
        DepositArgs,
    },
    state::action::ActionScope,
};
use roshi_interface::{access::MAX_ACCESS_PROOF_LEN, error::RoshiError};

/// Deposit base at NAV, mint shares, and relay exactly the deposited amount
/// through an admin-authorized Deploy action. The relay relocates custody and
/// does not change total_assets. Any failure rolls back the entire deposit.
///
/// Accounts 0..8 match Deposit's base-only layout, 8 is the deposit sub-account
/// PDA, 9 is the Action PDA, and 10.. is the CPI section. `accounts_start` is
/// relative to that section; the executable program follows the selected metas.
pub fn try_deposit_and_deploy<'info>(
    accounts: &'info [AccountInfo<'info>],
    args: DepositAndDeployArgs,
) -> ProgramResult {
    if args.access_proof.len() > MAX_ACCESS_PROOF_LEN {
        return Err(RoshiError::InvalidAccessProof.into());
    }
    let context = DepositAndDeployContext::load(accounts)?;
    let action = &context.validated.action;
    if action.scope != ActionScope::Deploy {
        return Err(RoshiError::UnauthorizedAction.into());
    }
    if decode_withdrawal_amount(&args.ix_data, action)? != args.amount {
        return Err(RoshiError::DeployAmountMismatch.into());
    }
    let authorized_cpi = validate_authorized_cpi(
        context.cpi_accounts,
        &context.validated,
        args.accounts_start,
        args.account_flags,
        args.ix_data,
    )?;
    authorized_cpi.require_all_metas_bound(&action.ops)?;

    let asset_mint = context.deposit.vault.base_mint;
    execute_deposit(
        context.deposit,
        DepositArgs {
            asset_mint,
            amount: args.amount,
            min_shares_out: args.min_shares_out,
            access_proof: args.access_proof,
        },
    )?;

    let custody = authorized_cpi.scan_subaccount_custody()?;
    invoke_authorized_cpi(&authorized_cpi)?;
    authorized_cpi.reverify_subaccount_custody(&custody)
}
