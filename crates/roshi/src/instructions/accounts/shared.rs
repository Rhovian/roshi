use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_system_interface::program as system_program;

pub(crate) fn next_account<'a, 'info>(
    accounts: &mut impl Iterator<Item = &'a AccountInfo<'info>>,
) -> Result<&'a AccountInfo<'info>, ProgramError> {
    accounts.next().ok_or(ProgramError::NotEnoughAccountKeys)
}

pub(super) fn require_writable_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    require_writable(account)
}

pub(crate) fn require_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

pub(super) fn require_uninitialized_account(account: &AccountInfo) -> ProgramResult {
    if !account.data_is_empty() || account.lamports() != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    Ok(())
}

pub(super) fn require_system_program(account: &AccountInfo) -> ProgramResult {
    if account.key != &system_program::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    Ok(())
}

/// Close `account`: move its lamports to `refund_to`, clear its data, and return
/// ownership to the system program.
pub(crate) fn close_account(account: &AccountInfo, refund_to: &AccountInfo) -> ProgramResult {
    let reclaimed = account.lamports();
    let refund_balance = refund_to.lamports();
    **refund_to.try_borrow_mut_lamports()? = refund_balance
        .checked_add(reclaimed)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    **account.try_borrow_mut_lamports()? = 0;

    account.resize(0)?;
    account.assign(&system_program::ID);

    Ok(())
}
