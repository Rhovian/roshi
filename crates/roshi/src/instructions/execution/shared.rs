use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use crate::{
    instructions::accounts::ValidatedManageAccounts,
    instructions::AccountFlags,
    state::{action::compute_action_hash_from_metas, sub_account::VaultSubAccount},
};
use roshi_interface::error::RoshiError;

pub(crate) struct AuthorizedCpi<'a> {
    instruction: Instruction,
    account_infos: Vec<AccountInfo<'a>>,
    vault_key: Pubkey,
    sub_account_key: Pubkey,
    sub_account_index: u8,
    sub_account_bump: u8,
}

impl AuthorizedCpi<'_> {
    pub(crate) fn has_account_meta(&self, key: &Pubkey) -> bool {
        self.instruction
            .accounts
            .iter()
            .any(|meta| &meta.pubkey == key)
    }

    /// Pre-CPI: identify writable custody accounts controlled by the subaccount
    /// and assert that each is clean before the downstream program runs.
    pub(crate) fn scan_subaccount_custody(&self) -> Result<Vec<Pubkey>, ProgramError> {
        let mut keys = Vec::new();
        for (meta, info) in self.instruction.accounts.iter().zip(&self.account_infos) {
            if meta.is_writable
                && crate::instructions::token::is_clean_custody(info, &self.sub_account_key)?
            {
                keys.push(*info.key);
            }
        }

        Ok(keys)
    }

    /// Post-CPI: re-check the pre-identified custody accounts by key.
    pub(crate) fn reverify_subaccount_custody(&self, keys: &[Pubkey]) -> ProgramResult {
        for key in keys {
            let info = self
                .account_infos
                .iter()
                .find(|info| info.key == key)
                .ok_or(ProgramError::from(RoshiError::InvalidTokenAccount))?;
            if !crate::instructions::token::is_clean_custody(info, &self.sub_account_key)? {
                return Err(RoshiError::InvalidTokenAccount.into());
            }
        }

        Ok(())
    }
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
/// Rebuilds the intended CPI metas from selected CPI accounts plus explicit
/// flags, then recomputes the action hash from the effective CPI program id,
/// stored `Ops`, rebuilt metas, and instruction data. The selected subaccount
/// is promoted to signer when present in the CPI metas.
pub(crate) fn validate_authorized_cpi<'a>(
    cpi_accounts: &[AccountInfo<'a>],
    validated_accounts: &ValidatedManageAccounts,
    program_id: [u8; 32],
    accounts_start: u8,
    accounts_len: u8,
    account_flags: Vec<AccountFlags>,
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
    if account_flags.len() != accounts_len {
        return Err(ProgramError::InvalidInstructionData);
    }

    let cpi_account_metas = cpi_meta_accounts
        .iter()
        .zip(account_flags)
        .map(|(acc, flags)| {
            if flags.is_writable && !acc.is_writable {
                return Err(ProgramError::InvalidAccountData);
            }

            let is_sub_account = acc.key == &validated_accounts.sub_account_key;
            if flags.is_signer && !acc.is_signer && !is_sub_account {
                return Err(ProgramError::MissingRequiredSignature);
            }

            let is_signer = flags.is_signer || is_sub_account;
            if flags.is_writable {
                Ok(AccountMeta::new(*acc.key, is_signer))
            } else {
                Ok(AccountMeta::new_readonly(*acc.key, is_signer))
            }
        })
        .collect::<Result<Vec<_>, ProgramError>>()?;

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
        sub_account_key: validated_accounts.sub_account_key,
        sub_account_index: validated_accounts.sub_account_index,
        sub_account_bump: validated_accounts.sub_account_bump,
    })
}

/// Invokes a CPI after all Roshi and CPI-specific authorization checks have
/// already been performed.
pub(crate) fn invoke_authorized_cpi(authorized_cpi: &AuthorizedCpi) -> ProgramResult {
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
