use solana_account_info::AccountInfo;
use solana_cpi::invoke;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use crate::state::program_config::ProgramConfig;

pub fn try_manage(
    accounts: &[AccountInfo],
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    ix_data: Vec<u8>,
) -> ProgramResult {
    let signer = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let config = accounts.get(1).ok_or(ProgramError::NotEnoughAccountKeys)?;
    ProgramConfig::verify_authority(config, signer)?;

    invoke_indexed_cpi(accounts, program_id, accounts_start, accounts_len, ix_data)
}

pub(crate) fn invoke_indexed_cpi(
    accounts: &[AccountInfo],
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    ix_data: Vec<u8>,
) -> ProgramResult {
    let accounts_start = usize::from(accounts_start);
    let accounts_len = usize::from(accounts_len);
    let accounts_end = accounts_start
        .checked_add(accounts_len)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let cpi_meta_accounts = accounts
        .get(accounts_start..accounts_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let cpi_program_id = Pubkey::from(program_id);
    let account_infos_end = if accounts
        .get(accounts_end)
        .is_some_and(|program_acc| program_acc.key == &cpi_program_id)
    {
        accounts_end
            .checked_add(1)
            .ok_or(ProgramError::InvalidInstructionData)?
    } else {
        accounts_end
    };
    let cpi_account_infos = accounts
        .get(accounts_start..account_infos_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    invoke(
        &Instruction {
            program_id: cpi_program_id,
            accounts: cpi_meta_accounts
                .iter()
                .map(|acc| AccountMeta {
                    pubkey: *acc.key,
                    is_signer: acc.is_signer,
                    is_writable: acc.is_writable,
                })
                .collect(),
            data: ix_data,
        },
        cpi_account_infos,
    )
}
