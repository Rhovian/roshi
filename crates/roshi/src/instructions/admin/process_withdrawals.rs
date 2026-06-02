use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;
use wincode::serialize;

use crate::{
    instructions::{
        accounts::next_account,
        token::{self, TOKEN_PROGRAM_ID},
        ProcessWithdrawalsArgs,
    },
    state::{
        sub_account::VaultSubAccount,
        vault::{Role, Vault},
        withdrawal_ticket::WithdrawalTicket,
        Account,
    },
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstructionTag::ProcessWithdrawals`].
///
/// # Accounts
///
/// 0. `[signer]` Vault withdrawal authority.
/// 1. `[writable]` Vault account containing withdrawal queue state.
/// 2. `[]` Withdraw subaccount PDA (`vault.withdraw_sub_account`).
/// 3. `[writable]` Withdraw subaccount base custody token account.
/// 4. `[]` SPL Token program.
/// 5. `..` Repeated `[writable]` withdrawal ticket, `[writable]` owner, and
///    `[writable]` destination token account groups.
///
/// Verifies the withdrawal authority, validates each supplied ticket, transfers
/// owed base assets from withdraw-subaccount custody to the recorded recipient,
/// closes settled tickets back to their owners, and reduces pending assets.
pub fn try_process_withdrawals(
    accounts: &[AccountInfo],
    _args: ProcessWithdrawalsArgs,
) -> ProgramResult {
    let context = ProcessWithdrawalsContext::load(accounts)?;
    context.process()
}

struct TicketSettlement<'a, 'info> {
    ticket_account: &'a AccountInfo<'info>,
    owner: &'a AccountInfo<'info>,
    destination: &'a AccountInfo<'info>,
    ticket: WithdrawalTicket,
}

struct ProcessWithdrawalsContext<'a, 'info> {
    vault_account: &'a AccountInfo<'info>,
    sub_account: &'a AccountInfo<'info>,
    custody: &'a AccountInfo<'info>,
    token_program: &'a AccountInfo<'info>,
    vault: Vault,
    sub_account_bump: u8,
    tickets: Vec<TicketSettlement<'a, 'info>>,
}

impl<'a, 'info> ProcessWithdrawalsContext<'a, 'info> {
    fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let withdrawal_authority = next_account(accounts_iter)?;
        let vault_account = next_account(accounts_iter)?;
        require_writable(vault_account)?;
        let vault = Account::load_as::<Vault>(vault_account)?;
        vault.verify_address(vault_account.key)?;
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

        let token_program = next_account(accounts_iter)?;
        if token_program.key != &TOKEN_PROGRAM_ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        let remaining = accounts_iter.as_slice();
        if remaining.is_empty() || remaining.len() % 3 != 0 {
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
                || ticket.assets_owed == 0
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
            token_program,
            vault,
            sub_account_bump,
            tickets,
        })
    }

    fn process(mut self) -> ProgramResult {
        let settled_assets = self.tickets.iter().try_fold(0u64, |sum, settlement| {
            sum.checked_add(settlement.ticket.assets_owed)
                .ok_or(ProgramError::from(RoshiError::Overflow))
        })?;
        if settled_assets > self.vault.pending_withdrawal_assets {
            return Err(RoshiError::InvalidVaultState.into());
        }

        let sub_account_bump = [self.sub_account_bump];
        let withdraw_sub_account = [self.vault.withdraw_sub_account];
        let signer_seeds: &[&[u8]] = &[
            VaultSubAccount::SEED,
            self.vault_account.key.as_ref(),
            &withdraw_sub_account,
            &sub_account_bump,
        ];

        for settlement in &self.tickets {
            token::transfer_signed(
                self.token_program,
                self.custody,
                settlement.destination,
                self.sub_account,
                settlement.ticket.assets_owed,
                signer_seeds,
            )?;
            close_ticket(settlement.ticket_account, settlement.owner)?;
        }

        self.vault.pending_withdrawal_assets = self
            .vault
            .pending_withdrawal_assets
            .checked_sub(settled_assets)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;

        self.store_vault()
    }

    fn store_vault(self) -> ProgramResult {
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

fn close_ticket(ticket_account: &AccountInfo, owner: &AccountInfo) -> ProgramResult {
    let reclaimed = ticket_account.lamports();
    let owner_balance = owner.lamports();
    **owner.try_borrow_mut_lamports()? = owner_balance
        .checked_add(reclaimed)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    **ticket_account.try_borrow_mut_lamports()? = 0;

    ticket_account.resize(0)?;
    ticket_account.assign(&system_program::ID);

    Ok(())
}

fn require_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}
