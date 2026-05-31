use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_system_interface::instruction::create_account;
use solana_sysvar::{rent::Rent, Sysvar};
use wincode::serialize;

use crate::{
    instructions::accounts::{
        next_account, require_system_program, require_uninitialized_account,
        require_writable_signer,
    },
    instructions::InitializeProgramArgs,
    state::{program_config::ProgramConfig, Account},
};

/// Implements [`crate::instructions::RoshiInstructionTag::InitializeProgram`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Payer that funds the program config account.
/// 1. `[writable]` Program config PDA derived from `ProgramConfig::SEED`.
/// 2. `[]` System program.
///
/// # Implementation
///
/// Validates the payer, rejects an already initialized config account, verifies
/// the system program, checks the config PDA seeds, creates the config account
/// with rent-exempt lamports, and stores the configured program authority.
pub fn try_initialize_program(
    accounts: &[AccountInfo],
    args: InitializeProgramArgs,
) -> ProgramResult {
    let accounts = InitializeProgramAccounts::parse(accounts)?;

    accounts.create_config_account()?;
    accounts.store_config(Pubkey::from(args.authority))
}

struct InitializeProgramAccounts<'a, 'info> {
    payer: &'a AccountInfo<'info>,
    program_config: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    program_config_bump: u8,
}

impl<'a, 'info> InitializeProgramAccounts<'a, 'info> {
    fn parse(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let payer = next_account(accounts_iter)?;
        require_writable_signer(payer)?;

        let program_config = next_account(accounts_iter)?;
        require_uninitialized_account(program_config)?;

        let system_program_acc = next_account(accounts_iter)?;
        require_system_program(system_program_acc)?;

        let program_config_bump = ProgramConfig::verify_address(program_config)?;

        Ok(Self {
            payer,
            program_config,
            system_program_acc,
            program_config_bump,
        })
    }

    fn create_config_account(&self) -> ProgramResult {
        let rent_exemption_lamports = Rent::get()?.minimum_balance(ProgramConfig::SPACE);
        let create_account_ix = create_account(
            self.payer.key,
            self.program_config.key,
            rent_exemption_lamports,
            ProgramConfig::SPACE as u64,
            &crate::ID,
        );
        let account_infos = [
            self.payer.clone(),
            self.program_config.clone(),
            self.system_program_acc.clone(),
        ];

        invoke_signed(
            &create_account_ix,
            &account_infos,
            &[&[ProgramConfig::SEED, &[self.program_config_bump]]],
        )
    }

    fn store_config(&self, authority: Pubkey) -> ProgramResult {
        let config = Account::ProgramConfig(ProgramConfig::new(authority));
        let serialized = serialize(&config).map_err(|_| ProgramError::InvalidAccountData)?;
        let mut data = self.program_config.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}
