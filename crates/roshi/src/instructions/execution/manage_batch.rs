use crate::instructions::IndexedActionArgs;
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use super::manage::invoke_authorized_cpi;

pub fn try_manage_batch(
    accounts: &[AccountInfo],
    actions: Vec<IndexedActionArgs>,
) -> ProgramResult {
    let strategist = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let vault = accounts.get(1).ok_or(ProgramError::NotEnoughAccountKeys)?;
    let action_count = actions.len();
    let cpi_accounts_base = 2usize
        .checked_add(
            action_count
                .checked_mul(2)
                .ok_or(ProgramError::InvalidInstructionData)?,
        )
        .ok_or(ProgramError::InvalidInstructionData)?;

    for (index, action) in actions.into_iter().enumerate() {
        let sub_account_acc = accounts
            .get(2 + index * 2)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        let action_acc = accounts
            .get(3 + index * 2)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;

        invoke_authorized_cpi(
            accounts,
            strategist,
            vault,
            sub_account_acc,
            action_acc,
            cpi_accounts_base,
            action.sub_account,
            action.program_id,
            action.accounts_start,
            action.accounts_len,
            action.ix_data,
        )?;
    }

    Ok(())
}
