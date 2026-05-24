use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_resume_vault(
    _accounts: &[AccountInfo],
    _deposits: bool,
    _withdrawals: bool,
) -> ProgramResult {
    Ok(())
}
