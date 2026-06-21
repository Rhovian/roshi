use crate::instructions::{accounts::ManageBatchContext, ManageBatchArgs};
use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use super::shared::{settle_authorized_cpi, validate_authorized_cpi};

/// Implements [`crate::instructions::RoshiInstruction::ManageBatch`].
///
/// # Accounts
///
/// 0. `[]` Action executor. Must be the vault strategist for manager actions;
///    public actions do not require an executor role.
/// 1. `[]` Vault account.
/// 2. `..` Repeated `(subaccount PDA, Action PDA)` pairs, one per action.
///    N. `..` Shared CPI account section after all pairs.
///
/// # Implementation
///
/// Consumes the executor, vault, and one `(subaccount PDA, Action PDA)` pair
/// per action from the front of the account list. The remaining accounts form
/// the shared CPI account section. Each action is validated and invoked in
/// order so account writes from earlier actions are visible to later action
/// validation. Each action selects its own subaccount and Action PDA pair while
/// using `accounts_start` and `accounts_len` as offsets into the shared CPI
/// accounts. The target CPI program account must follow each selected CPI
/// account meta slice.
pub fn try_manage_batch(accounts: &[AccountInfo], args: ManageBatchArgs) -> ProgramResult {
    let accounts = ManageBatchContext::load(accounts, args.actions.len())?;

    for (action, action_accounts) in args.actions.into_iter().zip(&accounts.action_accounts) {
        let validated_accounts = accounts.validate_action(action_accounts, action.sub_account)?;

        let authorized_cpi = validate_authorized_cpi(
            accounts.cpi_accounts,
            &validated_accounts,
            action.program_id,
            action.accounts_start,
            action.accounts_len,
            action.account_flags,
            action.ix_data,
        )?;
        settle_authorized_cpi(
            &authorized_cpi,
            &validated_accounts.action,
            accounts.cpi_accounts,
        )?;
    }

    Ok(())
}
