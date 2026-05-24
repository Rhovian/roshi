use crate::instructions::IndexedActionArgs;
use crate::state::program_config::ProgramConfig;
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use super::manage::invoke_indexed_cpi;

pub fn try_manage_batch(
    accounts: &[AccountInfo],
    actions: Vec<IndexedActionArgs>,
) -> ProgramResult {
    let signer = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let config = accounts.get(1).ok_or(ProgramError::NotEnoughAccountKeys)?;
    ProgramConfig::verify_authority(config, signer)?;

    for action in actions {
        invoke_indexed_cpi(
            accounts,
            action.program_id,
            action.accounts_start,
            action.accounts_len,
            action.ix_data,
        )?;
    }

    Ok(())
}
