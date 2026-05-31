use crate::instructions::{accounts::next_account, ManageBatchArgs};
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use super::shared::{
    invoke_authorized_cpi, validate_authorized_cpi, validate_manage_accounts,
    ValidatedManageAccounts,
};

/// Implements [`crate::instructions::RoshiInstructionTag::ManageBatch`].
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
pub fn try_manage_batch(accounts: &[AccountInfo], args: ManageBatchArgs) -> ProgramResult {
    let accounts = ManageBatchAccounts::parse(accounts, args.actions.len())?;

    for (action, action_accounts) in args.actions.into_iter().zip(&accounts.action_accounts) {
        let validated_accounts = accounts.validate_action(action_accounts, action.sub_account)?;

        let authorized_cpi = validate_authorized_cpi(
            accounts.cpi_accounts,
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

struct ManageBatchAccounts<'a, 'info> {
    strategist: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    action_accounts: Vec<ManageBatchActionAccounts<'a, 'info>>,
    cpi_accounts: &'a [AccountInfo<'info>],
}

impl<'a, 'info> ManageBatchAccounts<'a, 'info> {
    fn parse(accounts: &'a [AccountInfo<'info>], actions_len: usize) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();
        let strategist = next_account(accounts_iter)?;
        let vault = next_account(accounts_iter)?;

        let mut action_accounts = Vec::with_capacity(actions_len);
        for _ in 0..actions_len {
            let sub_account = next_account(accounts_iter)?;
            let action = next_account(accounts_iter)?;
            action_accounts.push(ManageBatchActionAccounts {
                sub_account,
                action,
            });
        }
        let cpi_accounts = accounts_iter.as_slice();

        Ok(Self {
            strategist,
            vault,
            action_accounts,
            cpi_accounts,
        })
    }

    fn validate_action(
        &self,
        action_accounts: &ManageBatchActionAccounts,
        sub_account_index: u8,
    ) -> Result<ValidatedManageAccounts, ProgramError> {
        validate_manage_accounts(
            self.strategist,
            self.vault,
            action_accounts.sub_account,
            action_accounts.action,
            sub_account_index,
        )
    }
}

struct ManageBatchActionAccounts<'a, 'info> {
    sub_account: &'a AccountInfo<'info>,
    action: &'a AccountInfo<'info>,
}
