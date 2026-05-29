use crate::instructions::IndexedActionArgs;
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use super::manage::{invoke_authorized_cpi, validate_authorized_cpi, validate_manage_accounts};

/// Implements [`crate::instructions::RoshiInstruction::ManageBatch`].
///
/// # Accounts
///
/// 0. `[signer]` Vault strategist.
/// 1. `[]` Vault account.
/// 2. `..` Repeated `(subaccount PDA, Action PDA)` pairs, one per action.
/// N. `..` Shared CPI account section after all pairs.
///
/// # Implementation
///
/// Consumes the strategist, vault, and one `(subaccount PDA, Action PDA)` pair
/// per action from the front of the account list. The remaining accounts form
/// the shared CPI account section. Each action is validated and invoked in
/// order so account writes from earlier actions are visible to later action
/// validation. Each action selects its own subaccount and Action PDA pair while
/// using `accounts_start` and `accounts_len` as offsets into the shared CPI
/// accounts. The target CPI program account must follow each selected CPI
/// account meta slice.
pub fn try_manage_batch(
    accounts: &[AccountInfo],
    actions: Vec<IndexedActionArgs>,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let strategist = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let vault = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    let mut action_accounts = Vec::with_capacity(actions.len());
    for _ in 0..actions.len() {
        let sub_account_acc = accounts_iter
            .next()
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        let action_acc = accounts_iter
            .next()
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        action_accounts.push((sub_account_acc, action_acc));
    }
    let cpi_accounts = accounts_iter.as_slice();

    for (action, (sub_account_acc, action_acc)) in
        actions.into_iter().zip(action_accounts.into_iter())
    {
        let validated_accounts = validate_manage_accounts(
            strategist,
            vault,
            sub_account_acc,
            action_acc,
            action.sub_account,
        )?;

        let authorized_cpi = validate_authorized_cpi(
            cpi_accounts,
            &validated_accounts,
            action.program_id,
            action.accounts_start,
            action.accounts_len,
            action.ix_data,
        )?;
        invoke_authorized_cpi(&authorized_cpi)?;
    }

    Ok(())
}
