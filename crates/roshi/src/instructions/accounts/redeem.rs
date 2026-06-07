use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_system_interface::instruction::create_account;
use solana_sysvar::{rent::Rent, Sysvar};
use wincode::serialize;

use super::shared::{
    next_account, require_system_program, require_uninitialized_account, require_writable,
    require_writable_signer,
};
use crate::{
    instructions::{token, RedeemArgs},
    state::{vault::Vault, withdrawal_ticket::WithdrawalTicket, Account},
};
use roshi_interface::error::RoshiError;

/// Fixed redeem account layout:
///
/// 0. `[signer, writable]` Share owner (redeemer; funds the ticket rent and
///    authorizes the share burn).
/// 1. `[writable]` Vault.
/// 2. `[writable]` Owner share token account (burn source).
/// 3. `[writable]` Share mint (`vault.share_mint`).
/// 4. `[]` Recipient base token account (payout destination).
/// 5. `[writable]` Withdrawal ticket PDA (uninitialized).
/// 6. `[]` System program.
/// 7. `[]` SPL Token program.
pub(crate) struct RedeemContext<'a, 'info> {
    pub(crate) owner: &'a AccountInfo<'info>,
    pub(crate) vault_account: &'a AccountInfo<'info>,
    pub(crate) share_source: &'a AccountInfo<'info>,
    pub(crate) share_mint: &'a AccountInfo<'info>,
    pub(crate) recipient_token_account: &'a AccountInfo<'info>,
    ticket: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    pub(crate) token_program: &'a AccountInfo<'info>,
    pub(crate) vault: Vault,
    ticket_index: u8,
    ticket_bump: u8,
}

impl<'a, 'info> RedeemContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        args: &RedeemArgs,
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let owner = next_account(accounts_iter)?;
        require_writable_signer(owner)?;
        let vault_account = next_account(accounts_iter)?;
        require_writable(vault_account)?;
        let share_source = next_account(accounts_iter)?;
        require_writable(share_source)?;
        let share_mint = next_account(accounts_iter)?;
        require_writable(share_mint)?;
        let recipient_token_account = next_account(accounts_iter)?;
        let ticket = next_account(accounts_iter)?;
        require_uninitialized_account(ticket)?;
        let system_program_acc = next_account(accounts_iter)?;
        require_system_program(system_program_acc)?;
        let token_program = next_account(accounts_iter)?;
        token::verify_token_program(token_program)?;

        let vault = Vault::load_checked(vault_account)?;
        vault.verify_share_mint(share_mint)?;
        let base_mint = Pubkey::from(vault.base_mint);
        if recipient_token_account.key != &Pubkey::from(args.recipient_token_account) {
            return Err(RoshiError::InvalidTokenAccount.into());
        }
        token::verify_token_account_mint(recipient_token_account, &base_mint)?;

        let (expected_ticket, ticket_bump) = WithdrawalTicket::find_address(
            vault_account.key,
            recipient_token_account.key,
            args.ticket_index,
        );
        if ticket.key != &expected_ticket {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Self {
            owner,
            vault_account,
            share_source,
            share_mint,
            recipient_token_account,
            ticket,
            system_program_acc,
            token_program,
            vault,
            ticket_index: args.ticket_index,
            ticket_bump,
        })
    }

    /// Create the rent-exempt withdrawal-ticket PDA (funded by the owner) and
    /// store `ticket`.
    pub(crate) fn create_ticket(&self, ticket: WithdrawalTicket) -> ProgramResult {
        let rent_exemption_lamports = Rent::get()?.minimum_balance(WithdrawalTicket::SPACE);
        let create_account_ix = create_account(
            self.owner.key,
            self.ticket.key,
            rent_exemption_lamports,
            WithdrawalTicket::SPACE as u64,
            &crate::ID,
        );
        let account_infos = [
            self.owner.clone(),
            self.ticket.clone(),
            self.system_program_acc.clone(),
        ];
        let bump = [self.ticket_bump];

        invoke_signed(
            &create_account_ix,
            &account_infos,
            &[&[
                WithdrawalTicket::SEED,
                self.vault_account.key.as_ref(),
                self.recipient_token_account.key.as_ref(),
                &[self.ticket_index],
                &bump,
            ]],
        )?;

        let serialized = serialize(&Account::WithdrawalTicket(ticket))
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if serialized.len() > WithdrawalTicket::SPACE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut data = self.ticket.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }

    pub(crate) fn ticket_bump(&self) -> u8 {
        self.ticket_bump
    }

    /// Apply `update` to the vault accounting and persist it.
    pub(crate) fn store(
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
