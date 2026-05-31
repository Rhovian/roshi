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
use crate::state::{program_config::ProgramConfig, Account};

pub(crate) struct ProgramConfigAuthorityContext<'a, 'info> {
    program_config_account: &'a AccountInfo<'info>,
    program_config: ProgramConfig,
}

impl<'a, 'info> ProgramConfigAuthorityContext<'a, 'info> {
    pub(crate) fn load(
        authority: &AccountInfo,
        program_config_account: &'a AccountInfo<'info>,
    ) -> Result<Self, ProgramError> {
        ProgramConfig::verify_address(program_config_account)?;

        let program_config = Account::load_as::<ProgramConfig>(program_config_account)?;
        if authority.key != &program_config.authority() {
            return Err(ProgramError::IllegalOwner);
        }

        if !authority.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        Ok(Self {
            program_config_account,
            program_config,
        })
    }
}

pub(crate) struct WritableProgramConfigAuthorityContext<'a, 'info> {
    program_config_account: &'a AccountInfo<'info>,
    program_config: ProgramConfig,
}

impl<'a, 'info> WritableProgramConfigAuthorityContext<'a, 'info> {
    pub(crate) fn load(
        authority: &AccountInfo,
        program_config_account: &'a AccountInfo<'info>,
    ) -> Result<Self, ProgramError> {
        require_writable(program_config_account)?;

        let context = ProgramConfigAuthorityContext::load(authority, program_config_account)?;

        Ok(Self {
            program_config_account: context.program_config_account,
            program_config: context.program_config,
        })
    }

    pub(crate) fn program_config_mut(&mut self) -> &mut ProgramConfig {
        &mut self.program_config
    }

    pub(crate) fn store(self) -> ProgramResult {
        let serialized = serialize(&Account::ProgramConfig(self.program_config))
            .map_err(|_| ProgramError::InvalidAccountData)?;
        let mut data = self.program_config_account.try_borrow_mut_data()?;
        if serialized.len() > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }

        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}

pub(crate) struct InitializeProgramContext<'a, 'info> {
    payer: &'a AccountInfo<'info>,
    program_config: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    program_config_bump: u8,
}

impl<'a, 'info> InitializeProgramContext<'a, 'info> {
    pub(crate) fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
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

    pub(crate) fn create_config_account(&self) -> ProgramResult {
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

    pub(crate) fn store_config(&self, authority: Pubkey) -> ProgramResult {
        let config = Account::ProgramConfig(ProgramConfig::new(authority));
        let serialized = serialize(&config).map_err(|_| ProgramError::InvalidAccountData)?;
        let mut data = self.program_config.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}
