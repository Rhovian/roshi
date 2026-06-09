use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use super::shared::{next_account, require_writable};
use crate::{
    instructions::token,
    state::{
        sub_account::VaultSubAccount,
        vault::{Role, Vault, VaultExt},
        withdrawal_ticket::WithdrawalTicket,
        Account,
    },
};
use roshi_interface::error::RoshiError;

/// One ticket queued for settlement: its account, the owner that funded its
/// rent, the payout destination, and the decoded ticket state.
pub(crate) struct TicketSettlement<'a, 'info> {
    pub(crate) ticket_account: &'a AccountInfo<'info>,
    pub(crate) owner: &'a AccountInfo<'info>,
    pub(crate) destination: &'a AccountInfo<'info>,
    pub(crate) ticket: WithdrawalTicket,
}

/// Loads `[withdrawal_authority signer, vault (writable), withdraw subaccount,
/// custody (writable), share mint, token program, (ticket, owner, destination)+]`,
/// verifies the withdrawal authority and every supplied ticket, and binds the
/// withdraw subaccount custody.
pub(crate) struct ProcessWithdrawalsContext<'a, 'info> {
    pub(crate) vault_account: &'a AccountInfo<'info>,
    pub(crate) sub_account: &'a AccountInfo<'info>,
    pub(crate) custody: &'a AccountInfo<'info>,
    pub(crate) share_mint: &'a AccountInfo<'info>,
    pub(crate) token_program: &'a AccountInfo<'info>,
    pub(crate) vault: Vault,
    pub(crate) sub_account_bump: u8,
    pub(crate) tickets: Vec<TicketSettlement<'a, 'info>>,
}

impl<'a, 'info> ProcessWithdrawalsContext<'a, 'info> {
    pub(crate) fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let withdrawal_authority = next_account(accounts_iter)?;
        let vault_account = next_account(accounts_iter)?;
        require_writable(vault_account)?;
        let vault = Vault::load_checked(vault_account)?;
        vault.verify_role(Role::WithdrawalAuthority, withdrawal_authority)?;

        let sub_account = next_account(accounts_iter)?;
        let sub_account_bump = VaultSubAccount::verify_account(
            vault_account.key,
            vault.withdraw_sub_account,
            sub_account,
        )?;

        let custody = next_account(accounts_iter)?;
        require_writable(custody)?;
        let base_mint = Pubkey::from(vault.base_mint);
        token::verify_token_account_mint_and_owner(custody, &base_mint, sub_account.key)?;

        let share_mint = next_account(accounts_iter)?;
        vault.verify_share_mint(share_mint)?;

        let token_program = next_account(accounts_iter)?;
        token::verify_token_program_for(token_program, custody)?;

        let remaining = accounts_iter.as_slice();
        if remaining.is_empty() || !remaining.len().is_multiple_of(3) {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let mut tickets = Vec::with_capacity(remaining.len() / 3);
        while !accounts_iter.as_slice().is_empty() {
            let ticket_account = next_account(accounts_iter)?;
            require_writable(ticket_account)?;
            let owner = next_account(accounts_iter)?;
            require_writable(owner)?;
            let destination = next_account(accounts_iter)?;
            require_writable(destination)?;

            let ticket = Account::load_as::<WithdrawalTicket>(ticket_account)?;
            if ticket.vault != vault_account.key.to_bytes()
                || ticket.owner != owner.key.to_bytes()
                || ticket.recipient_token_account != destination.key.to_bytes()
                || ticket.shares_burned == 0
            {
                return Err(RoshiError::InvalidWithdrawalTicketAccount.into());
            }

            let recipient = Pubkey::from(ticket.recipient_token_account);
            let (expected_ticket, expected_bump) =
                WithdrawalTicket::find_address(vault_account.key, &recipient, ticket.ticket_index);
            if ticket_account.key != &expected_ticket || ticket.bump != expected_bump {
                return Err(ProgramError::InvalidSeeds);
            }
            token::verify_token_account_mint(destination, &base_mint)?;
            if tickets.iter().any(|settlement: &TicketSettlement<'_, '_>| {
                settlement.ticket_account.key == ticket_account.key
            }) {
                return Err(RoshiError::InvalidWithdrawalTicketAccount.into());
            }

            tickets.push(TicketSettlement {
                ticket_account,
                owner,
                destination,
                ticket,
            });
        }

        Ok(Self {
            vault_account,
            sub_account,
            custody,
            share_mint,
            token_program,
            vault,
            sub_account_bump,
            tickets,
        })
    }

    /// Persist the mutated vault accounting.
    pub(crate) fn store_vault(self) -> ProgramResult {
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
