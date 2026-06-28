use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use super::shared::{settle_authorized_cpi, validate_authorized_cpi};
use crate::instructions::{accounts::ManageContext, ManageArgs};

/// Implements [`crate::instructions::RoshiInstruction::Manage`].
///
/// # Accounts
///
/// 0. `[]` Action executor. Must be the vault strategist for manager actions;
///    public actions do not require an executor role.
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
    let accounts = ManageContext::load(accounts)?;
    let validated_accounts = accounts.validate(args.sub_account)?;

    let authorized_cpi = validate_authorized_cpi(
        accounts.cpi_accounts,
        &validated_accounts,
        args.accounts_start,
        args.accounts_len,
        args.account_flags,
        args.ix_data,
    )?;

    settle_authorized_cpi(
        &authorized_cpi,
        &validated_accounts.action,
        accounts.cpi_accounts,
    )
}
