use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_system_interface::program as system_program;

pub(crate) fn next_account<'a, 'info>(
    accounts: &mut impl Iterator<Item = &'a AccountInfo<'info>>,
) -> Result<&'a AccountInfo<'info>, ProgramError> {
    accounts.next().ok_or(ProgramError::NotEnoughAccountKeys)
}

pub(crate) fn require_writable_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if !account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

pub(crate) fn require_uninitialized_account(account: &AccountInfo) -> ProgramResult {
    if !account.data_is_empty() || account.lamports() != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    Ok(())
}

pub(crate) fn require_system_program(account: &AccountInfo) -> ProgramResult {
    if account.key != &system_program::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    Ok(())
}
