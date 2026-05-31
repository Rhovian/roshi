use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use super::shared::{
    invoke_authorized_cpi, validate_authorized_cpi, validate_manage_accounts,
    ValidatedManageAccounts,
};
use crate::instructions::{accounts::next_account, ManageArgs};

/// Implements [`crate::instructions::RoshiInstructionTag::Manage`].
///
/// # Accounts
///
/// 0. `[signer]` Vault strategist.
/// 1. `[]` Vault account.
/// 2. `[]` Subaccount PDA derived from `(vault, sub_account)`.
/// 3. `[]` Action PDA derived from `(vault, recomputed_action_hash)`.
/// 4. `..` CPI account section. `accounts_start` is relative to this section,
///    and the target CPI program account must follow the selected CPI metas.
///
/// # Implementation
///
/// Consumes the fixed Roshi accounts from the front of the account list,
/// validates their expected shapes, validates the CPI authorization against the
/// remaining CPI account section, then invokes the prepared CPI.
pub fn try_manage(accounts: &[AccountInfo], args: ManageArgs) -> ProgramResult {
    let accounts = ManageAccounts::parse(accounts)?;
    let validated_accounts = accounts.validate(args.sub_account)?;

    let authorized_cpi = validate_authorized_cpi(
        accounts.cpi_accounts,
        &validated_accounts,
        args.program_id,
        args.accounts_start,
        args.accounts_len,
        args.ix_data,
    )?;

    invoke_authorized_cpi(&authorized_cpi)
}

struct ManageAccounts<'a, 'info> {
    strategist: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    sub_account: &'a AccountInfo<'info>,
    action: &'a AccountInfo<'info>,
    cpi_accounts: &'a [AccountInfo<'info>],
}

impl<'a, 'info> ManageAccounts<'a, 'info> {
    fn parse(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();
        let strategist = next_account(accounts_iter)?;
        let vault = next_account(accounts_iter)?;
        let sub_account = next_account(accounts_iter)?;
        let action = next_account(accounts_iter)?;
        let cpi_accounts = accounts_iter.as_slice();

        Ok(Self {
            strategist,
            vault,
            sub_account,
            action,
            cpi_accounts,
        })
    }

    fn validate(&self, sub_account_index: u8) -> Result<ValidatedManageAccounts, ProgramError> {
        validate_manage_accounts(
            self.strategist,
            self.vault,
            self.sub_account,
            self.action,
            sub_account_index,
        )
    }
}
