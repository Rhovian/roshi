use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;

use super::{shared::next_account, DepositContext, ValidatedManageAccounts};
use crate::{
    instructions::token,
    state::{action::Action, sub_account::VaultSubAccount, Account},
};

/// The eight base-deposit accounts, followed by the deposit sub-account PDA,
/// Action PDA, then the CPI section. There are no asset/oracle accounts.
pub(crate) struct DepositAndDeployContext<'a, 'info> {
    pub(crate) deposit: DepositContext<'a, 'info>,
    pub(crate) validated: ValidatedManageAccounts,
    pub(crate) cpi_accounts: &'a [AccountInfo<'info>],
}

impl<'a, 'info> DepositAndDeployContext<'a, 'info>
where
    'a: 'info,
{
    pub(crate) fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let deposit_accounts = accounts
            .get(..8)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        let deposit = DepositContext::load(deposit_accounts)?;
        let remaining = &mut accounts[8..].iter();
        let sub_account = next_account(remaining)?;
        let action_account = next_account(remaining)?;
        let vault_key = *deposit.vault_account.key;
        let sub_account_index = deposit.vault.deposit_sub_account;
        let sub_account_bump =
            VaultSubAccount::verify_account(&vault_key, sub_account_index, sub_account)?;
        token::verify_token_account_mint_and_owner(
            deposit.custody,
            &Pubkey::from(deposit.vault.base_mint),
            sub_account.key,
        )?;
        token::verify_custody_account(deposit.custody, sub_account.key)?;
        let action = Account::load_as::<Action>(action_account)?;
        action.verify_for_vault(&vault_key, action_account.key)?;

        Ok(Self {
            deposit,
            validated: ValidatedManageAccounts {
                action,
                vault_key,
                sub_account_key: *sub_account.key,
                sub_account_index,
                sub_account_bump,
            },
            cpi_accounts: remaining.as_slice(),
        })
    }
}
