use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::deserialize;

use crate::{
    error::RoshiError,
    state::{
        action::{compute_action_hash_from_metas, Action},
        sub_account::VaultSubAccount,
        vault::{Role, Vault},
        Account,
    },
};

const STRATEGIST_INDEX: usize = 0;
const VAULT_INDEX: usize = 1;
const SUB_ACCOUNT_INDEX: usize = 2;
const ACTION_INDEX: usize = 3;
const CPI_ACCOUNTS_BASE: usize = 4;

pub fn try_manage(
    accounts: &[AccountInfo],
    sub_account: u8,
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    ix_data: Vec<u8>,
) -> ProgramResult {
    let strategist = accounts
        .get(STRATEGIST_INDEX)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let vault = accounts
        .get(VAULT_INDEX)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let sub_account_acc = accounts
        .get(SUB_ACCOUNT_INDEX)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let action = accounts
        .get(ACTION_INDEX)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    invoke_authorized_cpi(
        accounts,
        strategist,
        vault,
        sub_account_acc,
        action,
        CPI_ACCOUNTS_BASE,
        sub_account,
        program_id,
        accounts_start,
        accounts_len,
        ix_data,
    )
}

pub(crate) fn invoke_authorized_cpi(
    accounts: &[AccountInfo],
    strategist_acc: &AccountInfo,
    vault_acc: &AccountInfo,
    sub_account_acc: &AccountInfo,
    action_acc: &AccountInfo,
    cpi_accounts_base: usize,
    sub_account_index: u8,
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    ix_data: Vec<u8>,
) -> ProgramResult {
    let vault = load_vault(vault_acc)?;
    verify_role(&vault, Role::Strategist, strategist_acc)?;
    verify_manage_enabled(&vault)?;
    let sub_account_bump = verify_sub_account(vault_acc, sub_account_acc, sub_account_index)?;

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
    let cpi_account_metas = cpi_meta_accounts
        .iter()
        .map(|acc| {
            let is_signer = acc.is_signer || acc.key == sub_account_acc.key;
            if acc.is_writable {
                AccountMeta::new(*acc.key, is_signer)
            } else {
                AccountMeta::new_readonly(*acc.key, is_signer)
            }
        })
        .collect::<Vec<_>>();

    let action_hash =
        compute_action_hash_from_metas(&cpi_program_id, &action.ops, &cpi_account_metas, &ix_data)?;
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

    let sub_account_index_seed = [sub_account_index];
    let sub_account_bump_seed = [sub_account_bump];
    let signer_seeds = &[
        VaultSubAccount::SEED,
        vault_acc.key.as_ref(),
        &sub_account_index_seed,
        &sub_account_bump_seed,
    ];

    invoke_signed(
        &Instruction {
            program_id: cpi_program_id,
            accounts: cpi_account_metas,
            data: ix_data,
        },
        cpi_account_infos,
        &[signer_seeds],
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

fn verify_role(vault: &Vault, role: Role, signer_acc: &AccountInfo) -> ProgramResult {
    if !signer_acc.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if !vault.has_role(role, signer_acc.key) {
        return Err(ProgramError::IllegalOwner);
    }

    Ok(())
}

fn verify_sub_account(
    vault_acc: &AccountInfo,
    sub_account_acc: &AccountInfo,
    sub_account_index: u8,
) -> Result<u8, ProgramError> {
    let (expected_sub_account_key, sub_account_bump) =
        VaultSubAccount::find_address(vault_acc.key, sub_account_index);
    if sub_account_acc.key != &expected_sub_account_key {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(sub_account_bump)
}

fn verify_manage_enabled(vault: &Vault) -> ProgramResult {
    if vault.manage_paused {
        return Err(RoshiError::VaultPaused.into());
    }

    Ok(())
}
