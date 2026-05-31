use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use crate::{
    error::RoshiError,
    state::{
        action::{compute_action_hash_from_metas, Action},
        sub_account::VaultSubAccount,
        vault::{Role, Vault},
        Account,
    },
};

pub(super) struct AuthorizedCpi<'a> {
    instruction: Instruction,
    account_infos: Vec<AccountInfo<'a>>,
    vault_key: Pubkey,
    sub_account_index: u8,
    sub_account_bump: u8,
}

/// Validates and prepares one pre-authorized downstream CPI.
///
/// # Accounts
///
/// `cpi_accounts` is the remaining account section after the Roshi instruction
/// prefix has been consumed. `accounts_start` and `accounts_len` select the
/// downstream CPI account metas relative to that section. The target program
/// account must be supplied immediately after the selected CPI account metas;
/// it must be executable and is passed through to `invoke_signed` as an
/// account info but is not included as an instruction meta.
///
/// # Implementation
///
/// Recomputes the action hash from the effective CPI program id, stored `Ops`,
/// selected CPI account metas, and instruction data, then promotes the selected
/// subaccount to signer when present in the CPI metas.
pub(super) fn validate_authorized_cpi<'a>(
    cpi_accounts: &[AccountInfo<'a>],
    validated_accounts: &ValidatedManageAccounts,
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    ix_data: Vec<u8>,
) -> Result<AuthorizedCpi<'a>, ProgramError> {
    let accounts_start = usize::from(accounts_start);
    let accounts_len = usize::from(accounts_len);
    let accounts_end = accounts_start
        .checked_add(accounts_len)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let cpi_meta_accounts = cpi_accounts
        .get(accounts_start..accounts_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let cpi_program_id = Pubkey::from(program_id);
    let cpi_account_metas = cpi_meta_accounts
        .iter()
        .map(|acc| {
            let is_signer = acc.is_signer || acc.key == &validated_accounts.sub_account_key;
            if acc.is_writable {
                AccountMeta::new(*acc.key, is_signer)
            } else {
                AccountMeta::new_readonly(*acc.key, is_signer)
            }
        })
        .collect::<Vec<_>>();

    let action_hash = compute_action_hash_from_metas(
        &cpi_program_id,
        &validated_accounts.action.ops,
        &cpi_account_metas,
        &ix_data,
    )?;
    if validated_accounts.action.action_hash != action_hash {
        return Err(RoshiError::UnauthorizedAction.into());
    }

    let cpi_program_acc = cpi_accounts
        .get(accounts_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    if cpi_program_acc.key != &cpi_program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !cpi_program_acc.executable {
        return Err(ProgramError::InvalidAccountData);
    }

    let account_infos_end = accounts_end
        .checked_add(1)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let cpi_account_infos = cpi_accounts
        .get(accounts_start..account_infos_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    Ok(AuthorizedCpi {
        instruction: Instruction {
            program_id: cpi_program_id,
            accounts: cpi_account_metas,
            data: ix_data,
        },
        account_infos: cpi_account_infos.to_vec(),
        vault_key: validated_accounts.vault_key,
        sub_account_index: validated_accounts.sub_account_index,
        sub_account_bump: validated_accounts.sub_account_bump,
    })
}

/// Invokes a CPI after all Roshi and CPI-specific authorization checks have
/// already been performed.
pub(super) fn invoke_authorized_cpi(authorized_cpi: &AuthorizedCpi) -> ProgramResult {
    let sub_account_index_seed = [authorized_cpi.sub_account_index];
    let sub_account_bump_seed = [authorized_cpi.sub_account_bump];
    let signer_seeds = &[
        VaultSubAccount::SEED,
        authorized_cpi.vault_key.as_ref(),
        &sub_account_index_seed,
        &sub_account_bump_seed,
    ];

    invoke_signed(
        &authorized_cpi.instruction,
        &authorized_cpi.account_infos,
        &[signer_seeds],
    )
}

pub(super) struct ValidatedManageAccounts {
    action: Action,
    vault_key: Pubkey,
    sub_account_key: Pubkey,
    sub_account_index: u8,
    sub_account_bump: u8,
}

/// Validates the fixed Roshi account prefix for one manage action.
///
/// # Checks
///
/// - `vault_acc` must be a Roshi `Vault` account.
/// - `vault_acc` must be the PDA for the loaded vault's `(admin, base_mint)`.
/// - `strategist_acc` must sign and match the vault's strategist role.
/// - `sub_account_acc` must be the PDA for `(vault_acc, sub_account_index)`.
/// - `sub_account_acc` must be a non-executable system account with no data.
/// - `action_acc` must be a Roshi `Action` account.
/// - `action_acc` must be scoped to `vault_acc` and use the expected Action
///   PDA seeds and bump.
/// - the vault must not have `manage_paused` set.
///
/// Returns the loaded Action and subaccount PDA bump needed by CPI execution.
pub(super) fn validate_manage_accounts(
    strategist_acc: &AccountInfo,
    vault_acc: &AccountInfo,
    sub_account_acc: &AccountInfo,
    action_acc: &AccountInfo,
    sub_account_index: u8,
) -> Result<ValidatedManageAccounts, ProgramError> {
    let vault = Account::load_as::<Vault>(vault_acc)?;
    vault.verify_address(vault_acc.key)?;
    vault.verify_role(Role::Strategist, strategist_acc)?;

    let sub_account_bump =
        VaultSubAccount::verify_account(vault_acc.key, sub_account_index, sub_account_acc)?;
    let action = Account::load_as::<Action>(action_acc)?;
    action.verify_for_vault(vault_acc.key, action_acc.key)?;

    vault.verify_manage_enabled()?;

    Ok(ValidatedManageAccounts {
        action,
        vault_key: *vault_acc.key,
        sub_account_key: *sub_account_acc.key,
        sub_account_index,
        sub_account_bump,
    })
}
