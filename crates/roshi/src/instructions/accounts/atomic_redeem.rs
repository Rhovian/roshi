use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use super::shared::{next_account, require_writable, require_writable_signer};
use crate::{
    instructions::{token, AtomicRedeemArgs},
    state::{
        action::Action,
        sub_account::VaultSubAccount,
        vault::{self, Vault},
        Account,
    },
};

/// Fixed atomic-redeem account layout:
///
/// 0. `[signer, writable]` Owner (redeeming user; authorizes the share burn).
/// 1. `[writable]` Vault.
/// 2. `[writable]` Owner share token account (burn source; mint = share mint,
///    owner = signer).
/// 3. `[writable]` Share mint (`vault.share_mint`).
/// 4. `[writable]` Recipient base token account (payout destination; mint =
///    base mint).
/// 5. `[writable]` Vault base custody token account (mint = base mint, owner =
///    sub_account PDA).
/// 6. `[]` Base SPL Token program.
/// 7. `[]` Subaccount PDA derived from `(vault, sub_account)`.
/// 8. `[]` Action PDA derived from `(vault, recomputed_action_hash)`.
/// 9. `[]` Classic SPL Token program.
/// 10. `..` CPI account section.
pub(crate) struct AtomicRedeemContext<'a, 'info> {
    pub(crate) owner: &'a AccountInfo<'info>,
    pub(crate) vault_account: &'a AccountInfo<'info>,
    pub(crate) user_share_account: &'a AccountInfo<'info>,
    pub(crate) share_mint: &'a AccountInfo<'info>,
    pub(crate) recipient_token_account: &'a AccountInfo<'info>,
    pub(crate) custody: &'a AccountInfo<'info>,
    pub(crate) base_token_program: &'a AccountInfo<'info>,
    pub(crate) sub_account: &'a AccountInfo<'info>,
    pub(crate) token_program: &'a AccountInfo<'info>,
    pub(crate) cpi_accounts: &'a [AccountInfo<'info>],
    pub(crate) vault: Vault,
    pub(crate) action: Box<Action>,
    pub(crate) sub_account_bump: u8,
}

impl<'a, 'info> AtomicRedeemContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        args: &AtomicRedeemArgs,
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let owner = next_account(accounts_iter)?;
        require_writable_signer(owner)?;
        let vault_account = next_account(accounts_iter)?;
        require_writable(vault_account)?;
        let user_share_account = next_account(accounts_iter)?;
        require_writable(user_share_account)?;
        let share_mint = next_account(accounts_iter)?;
        require_writable(share_mint)?;
        let recipient_token_account = next_account(accounts_iter)?;
        require_writable(recipient_token_account)?;
        let custody = next_account(accounts_iter)?;
        require_writable(custody)?;
        let base_token_program = next_account(accounts_iter)?;
        let sub_account = next_account(accounts_iter)?;
        let action_account = next_account(accounts_iter)?;
        let token_program = next_account(accounts_iter)?;
        if token_program.key != &token::TOKEN_PROGRAM_ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        let cpi_accounts = accounts_iter.as_slice();

        let vault = vault::load_checked(vault_account)?;
        let vault_key = *vault_account.key;

        vault::verify_share_mint(&vault, share_mint)?;
        let share_mint_key = Pubkey::from(vault.share_mint);
        token::verify_token_account_mint_and_owner(user_share_account, &share_mint_key, owner.key)?;

        let base_mint = Pubkey::from(vault.base_mint);
        token::verify_token_account_mint(recipient_token_account, &base_mint)?;
        token::verify_token_program_for(base_token_program, recipient_token_account)?;
        let sub_account_bump =
            VaultSubAccount::verify_account(&vault_key, args.sub_account, sub_account)?;
        token::verify_token_account_mint_and_owner(custody, &base_mint, sub_account.key)?;
        token::verify_token_program_for(base_token_program, custody)?;
        token::verify_custody_account(custody, sub_account.key)?;

        let action = Account::load_as::<Action>(action_account)?;
        action.verify_for_vault(&vault_key, action_account.key)?;

        Ok(Self {
            owner,
            vault_account,
            user_share_account,
            share_mint,
            recipient_token_account,
            custody,
            base_token_program,
            sub_account,
            token_program,
            cpi_accounts,
            vault,
            action: Box::new(action),
            sub_account_bump,
        })
    }

    /// Persist the mutated vault accounting.
    pub(crate) fn store_vault(&self) -> ProgramResult {
        self.vault.validate_state()?;

        let serialized =
            serialize(&Account::Vault(self.vault)).map_err(|_| ProgramError::InvalidAccountData)?;
        let mut data = self.vault_account.try_borrow_mut_data()?;
        if serialized.len() > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}
