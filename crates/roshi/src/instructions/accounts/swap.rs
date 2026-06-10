use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;

use super::shared::{next_account, require_writable};
use crate::{
    instructions::{token, SwapArgs},
    state::{
        action::{Action, ActionScope},
        sub_account::VaultSubAccount,
        vault::{self, Role},
        Account,
    },
};
use roshi_interface::error::RoshiError;

/// Fixed swap account layout:
///
/// 0. `[signer]` Swap authority (verified against `vault.swap_authority`).
/// 1. `[]` Vault.
/// 2. `[]` Subaccount PDA derived from `(vault, sub_account)`.
/// 3. `[writable]` Input custody token account (owner = subaccount PDA).
/// 4. `[writable]` Output custody token account (owner = subaccount PDA).
/// 5. `[]` Action PDA derived from `(vault, recomputed_action_hash)`.
/// 6. `..` CPI account section.
pub(crate) struct SwapContext<'a, 'info> {
    pub(crate) sub_account: &'a AccountInfo<'info>,
    pub(crate) input_custody: &'a AccountInfo<'info>,
    pub(crate) output_custody: &'a AccountInfo<'info>,
    pub(crate) cpi_accounts: &'a [AccountInfo<'info>],
    pub(crate) action: Action,
    pub(crate) vault_key: Pubkey,
    pub(crate) sub_account_index: u8,
    pub(crate) sub_account_bump: u8,
}

impl<'a, 'info> SwapContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        args: &SwapArgs,
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let swap_authority = next_account(accounts_iter)?;
        let vault_account = next_account(accounts_iter)?;
        let vault = vault::load_checked(vault_account)?;
        vault::verify_role(&vault, Role::SwapAuthority, swap_authority)?;
        vault.verify_manage_enabled()?;

        let vault_key = *vault_account.key;
        let sub_account = next_account(accounts_iter)?;
        let sub_account_bump =
            VaultSubAccount::verify_account(&vault_key, args.sub_account, sub_account)?;

        let input_custody = next_account(accounts_iter)?;
        require_writable(input_custody)?;
        token::verify_custody_account(input_custody, sub_account.key)?;

        let output_custody = next_account(accounts_iter)?;
        require_writable(output_custody)?;
        token::verify_custody_account(output_custody, sub_account.key)?;

        if input_custody.key == output_custody.key {
            return Err(RoshiError::InvalidTokenAccount.into());
        }

        let action_account = next_account(accounts_iter)?;
        let action = Account::load_as::<Action>(action_account)?;
        action.verify_for_vault(&vault_key, action_account.key)?;
        if action.scope != ActionScope::Swap {
            return Err(RoshiError::UnauthorizedAction.into());
        }

        let cpi_accounts = accounts_iter.as_slice();

        Ok(Self {
            sub_account,
            input_custody,
            output_custody,
            cpi_accounts,
            action,
            vault_key,
            sub_account_index: args.sub_account,
            sub_account_bump,
        })
    }
}
