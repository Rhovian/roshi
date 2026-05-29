use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_system_interface::{instruction::create_account, program as system_program};
use solana_sysvar::{rent::Rent, Sysvar};
use wincode::serialize;

use crate::state::{program_config::ProgramConfig, Account};

/// Implements [`crate::instructions::RoshiInstruction::InitializeProgram`].
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
pub fn try_initialize_program(accounts: &[AccountInfo], authority: [u8; 32]) -> ProgramResult {
    let mut accounts_iter = accounts.iter();

    let payer = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !payer.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    let program_config = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    if !program_config.data_is_empty() || program_config.lamports() != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    let system_program_acc = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    if system_program_acc.key.to_bytes() != system_program::ID.to_bytes() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (expected_program_config_key, program_config_bump) = ProgramConfig::find_address();
    if program_config.key != &expected_program_config_key {
        return Err(ProgramError::InvalidSeeds);
    }

    let rent_exemption_lamports = Rent::get()?.minimum_balance(ProgramConfig::SPACE);
    let create_account_ix = create_account(
        payer.key,
        program_config.key,
        rent_exemption_lamports,
        ProgramConfig::SPACE as u64,
        &crate::ID,
    );

    invoke_signed(
        &create_account_ix,
        accounts,
        &[&[ProgramConfig::SEED, &[program_config_bump]]],
    )?;

    let config = Account::ProgramConfig(ProgramConfig::new(Pubkey::from(authority)));
    let serialized = serialize(&config).map_err(|_| ProgramError::InvalidAccountData)?;
    program_config.try_borrow_mut_data()?[..serialized.len()].copy_from_slice(&serialized);

    Ok(())
}
