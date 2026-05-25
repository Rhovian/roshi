use solana_account_info::AccountInfo;
use solana_cpi::invoke;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::deserialize;

use crate::{
    error::RoshiError,
    state::{
        action::{compute_action_hash, Action},
        vault::Vault,
        Account,
    },
};

const OPERATOR_INDEX: usize = 0;
const VAULT_INDEX: usize = 1;
const ACTION_INDEX: usize = 2;
const CPI_ACCOUNTS_BASE: usize = 3;

pub fn try_manage(
    accounts: &[AccountInfo],
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    ix_data: Vec<u8>,
) -> ProgramResult {
    let operator = accounts
        .get(OPERATOR_INDEX)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let vault = accounts
        .get(VAULT_INDEX)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let action = accounts
        .get(ACTION_INDEX)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    invoke_authorized_cpi(
        accounts,
        operator,
        vault,
        action,
        CPI_ACCOUNTS_BASE,
        program_id,
        accounts_start,
        accounts_len,
        ix_data,
    )
}

pub(crate) fn invoke_authorized_cpi(
    accounts: &[AccountInfo],
    operator_acc: &AccountInfo,
    vault_acc: &AccountInfo,
    action_acc: &AccountInfo,
    cpi_accounts_base: usize,
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    ix_data: Vec<u8>,
) -> ProgramResult {
    let vault = load_vault(vault_acc)?;
    verify_operator(&vault, operator_acc)?;

    let action = load_action(action_acc)?;
    if action.vault != vault_acc.key.to_bytes() {
        return Err(RoshiError::UnauthorizedAction.into());
    }

    let accounts_start = cpi_accounts_base
        .checked_add(usize::from(accounts_start))
        .ok_or(ProgramError::InvalidInstructionData)?;
    let accounts_len = usize::from(accounts_len);
    let accounts_end = accounts_start
        .checked_add(accounts_len)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let cpi_meta_accounts = accounts
        .get(accounts_start..accounts_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let cpi_program_id = Pubkey::from(program_id);

    let action_hash =
        compute_action_hash(&cpi_program_id, &action.ops, cpi_meta_accounts, &ix_data)?;
    if action.action_hash != action_hash {
        return Err(RoshiError::UnauthorizedAction.into());
    }

    let (expected_action_key, _) = Action::find_address(vault_acc.key, &action_hash);
    if action_acc.key != &expected_action_key {
        return Err(ProgramError::InvalidSeeds);
    }

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

fn load_vault(vault_acc: &AccountInfo) -> Result<Vault, ProgramError> {
    if vault_acc.owner != &crate::ID {
        return Err(ProgramError::IllegalOwner);
    }

    let vault_data = vault_acc.data.borrow();
    match deserialize(&vault_data).map_err(|_| ProgramError::InvalidAccountData)? {
        Account::Vault(vault) => Ok(vault),
        _ => Err(ProgramError::InvalidAccountData),
    }
}

fn load_action(action_acc: &AccountInfo) -> Result<Action, ProgramError> {
    if action_acc.owner != &crate::ID {
        return Err(ProgramError::IllegalOwner);
    }

    let action_data = action_acc.data.borrow();
    match deserialize(&action_data).map_err(|_| ProgramError::InvalidAccountData)? {
        Account::Action(action) => Ok(action),
        _ => Err(ProgramError::InvalidAccountData),
    }
}

fn verify_operator(vault: &Vault, operator_acc: &AccountInfo) -> ProgramResult {
    if !operator_acc.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if operator_acc.key.to_bytes() != vault.operator {
        return Err(ProgramError::IllegalOwner);
    }

    Ok(())
}
