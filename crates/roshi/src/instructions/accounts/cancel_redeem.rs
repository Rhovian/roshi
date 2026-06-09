use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use super::shared::{close_account, next_account, require_writable, require_writable_signer};
use crate::{
    instructions::token,
    state::{vault::{Vault, VaultExt}, withdrawal_ticket::WithdrawalTicket, Account},
};
use roshi_interface::error::RoshiError;

/// Fixed cancel-redeem account layout:
///
/// 0. `[signer, writable]` Original share owner cancelling the ticket and
///    receiving reclaimed ticket rent.
/// 1. `[writable]` Vault.
/// 2. `[writable]` Withdrawal ticket PDA.
/// 3. `[writable]` Share mint (`vault.share_mint`).
/// 4. `[writable]` Owner share token account receiving reminted shares.
/// 5. `[]` SPL Token program.
pub(crate) struct CancelRedeemContext<'a, 'info> {
    pub(crate) owner: &'a AccountInfo<'info>,
    pub(crate) vault_account: &'a AccountInfo<'info>,
    ticket_account: &'a AccountInfo<'info>,
    pub(crate) share_mint: &'a AccountInfo<'info>,
    pub(crate) share_dest: &'a AccountInfo<'info>,
    pub(crate) token_program: &'a AccountInfo<'info>,
    pub(crate) vault: Vault,
    pub(crate) ticket: WithdrawalTicket,
}

impl<'a, 'info> CancelRedeemContext<'a, 'info> {
    pub(crate) fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let owner = next_account(accounts_iter)?;
        require_writable_signer(owner)?;
        let vault_account = next_account(accounts_iter)?;
        require_writable(vault_account)?;
        let ticket_account = next_account(accounts_iter)?;
        require_writable(ticket_account)?;
        let share_mint = next_account(accounts_iter)?;
        require_writable(share_mint)?;
        let share_dest = next_account(accounts_iter)?;
        require_writable(share_dest)?;
        let token_program = next_account(accounts_iter)?;
        if token_program.key != &token::TOKEN_PROGRAM_ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        let vault = Vault::load_checked(vault_account)?;
        vault.verify_share_mint(share_mint)?;

        let ticket = Account::load_as::<WithdrawalTicket>(ticket_account)?;
        if ticket.vault != vault_account.key.to_bytes() || ticket.owner != owner.key.to_bytes() {
            return Err(RoshiError::InvalidWithdrawalTicketAccount.into());
        }
        let recipient = Pubkey::from(ticket.recipient_token_account);
        let (expected_ticket, expected_bump) =
            WithdrawalTicket::find_address(vault_account.key, &recipient, ticket.ticket_index);
        if ticket_account.key != &expected_ticket || ticket.bump != expected_bump {
            return Err(ProgramError::InvalidSeeds);
        }
        token::verify_token_account_mint_and_owner(share_dest, share_mint.key, owner.key)?;

        Ok(Self {
            owner,
            vault_account,
            ticket_account,
            share_mint,
            share_dest,
            token_program,
            vault,
            ticket,
        })
    }

    pub(crate) fn close_ticket(&self) -> ProgramResult {
        close_account(self.ticket_account, self.owner)
    }

    pub(crate) fn store_vault(
        mut self,
        update: impl FnOnce(&mut Vault) -> ProgramResult,
    ) -> ProgramResult {
        update(&mut self.vault)?;

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
