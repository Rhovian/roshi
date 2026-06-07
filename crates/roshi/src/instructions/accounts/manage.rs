use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;

use super::shared::next_account;
use crate::state::{
    action::{Action, ActionScope},
    sub_account::VaultSubAccount,
    vault::{Role, Vault},
    Account,
};
use roshi_interface::error::RoshiError;

pub(crate) struct ValidatedManageAccounts {
    pub(crate) action: Action,
    pub(crate) vault_key: Pubkey,
    pub(crate) sub_account_key: Pubkey,
    pub(crate) sub_account_index: u8,
    pub(crate) sub_account_bump: u8,
}

pub(crate) struct ManageContext<'a, 'info> {
    executor: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    sub_account: &'a AccountInfo<'info>,
    action: &'a AccountInfo<'info>,
    pub(crate) cpi_accounts: &'a [AccountInfo<'info>],
}

impl<'a, 'info> ManageContext<'a, 'info> {
    pub(crate) fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();
        let executor = next_account(accounts_iter)?;
        let vault = next_account(accounts_iter)?;
        let sub_account = next_account(accounts_iter)?;
        let action = next_account(accounts_iter)?;
        let cpi_accounts = accounts_iter.as_slice();

        Ok(Self {
            executor,
            vault,
            sub_account,
            action,
            cpi_accounts,
        })
    }

    pub(crate) fn validate(
        &self,
        sub_account_index: u8,
    ) -> Result<ValidatedManageAccounts, ProgramError> {
        validate_manage_accounts(
            self.executor,
            self.vault,
            self.sub_account,
            self.action,
            sub_account_index,
        )
    }
}

pub(crate) struct ManageBatchContext<'a, 'info> {
    executor: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    pub(crate) action_accounts: Vec<ManageBatchActionContext<'a, 'info>>,
    pub(crate) cpi_accounts: &'a [AccountInfo<'info>],
}

impl<'a, 'info> ManageBatchContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        actions_len: usize,
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();
        let executor = next_account(accounts_iter)?;
        let vault = next_account(accounts_iter)?;

        let mut action_accounts = Vec::with_capacity(actions_len);
        for _ in 0..actions_len {
            let sub_account = next_account(accounts_iter)?;
            let action = next_account(accounts_iter)?;
            action_accounts.push(ManageBatchActionContext {
                sub_account,
                action,
            });
        }
        let cpi_accounts = accounts_iter.as_slice();

        Ok(Self {
            executor,
            vault,
            action_accounts,
            cpi_accounts,
        })
    }

    pub(crate) fn validate_action(
        &self,
        action_accounts: &ManageBatchActionContext,
        sub_account_index: u8,
    ) -> Result<ValidatedManageAccounts, ProgramError> {
        validate_manage_accounts(
            self.executor,
            self.vault,
            action_accounts.sub_account,
            action_accounts.action,
            sub_account_index,
        )
    }
}

pub(crate) struct ManageBatchActionContext<'a, 'info> {
    sub_account: &'a AccountInfo<'info>,
    action: &'a AccountInfo<'info>,
}

fn validate_manage_accounts(
    executor_acc: &AccountInfo,
    vault_acc: &AccountInfo,
    sub_account_acc: &AccountInfo,
    action_acc: &AccountInfo,
    sub_account_index: u8,
) -> Result<ValidatedManageAccounts, ProgramError> {
    let vault = Account::load_as::<Vault>(vault_acc)?;
    vault.verify_address(vault_acc.key)?;
    let vault_key = *vault_acc.key;

    let sub_account_bump =
        VaultSubAccount::verify_account(&vault_key, sub_account_index, sub_account_acc)?;
    let action = Account::load_as::<Action>(action_acc)?;
    action.verify_for_vault(&vault_key, action_acc.key)?;

    verify_action_executor(&vault, executor_acc, action.scope)?;
    vault.verify_manage_enabled()?;

    Ok(ValidatedManageAccounts {
        action,
        vault_key,
        sub_account_key: *sub_account_acc.key,
        sub_account_index,
        sub_account_bump,
    })
}

fn verify_action_executor(
    vault: &Vault,
    executor: &AccountInfo,
    scope: ActionScope,
) -> Result<(), ProgramError> {
    match scope {
        ActionScope::Manager => vault.verify_role(Role::Strategist, executor),
        ActionScope::Swap => vault.verify_role(Role::SwapAuthority, executor),
        ActionScope::AtomicRedeem => Err(RoshiError::UnauthorizedAction.into()),
    }
}
