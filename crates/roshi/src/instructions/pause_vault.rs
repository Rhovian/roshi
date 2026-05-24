use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_pause_vault(
    _accounts: &[AccountInfo],
    _deposits_paused: bool,
    _withdrawals_paused: bool,
) -> ProgramResult {
    Ok(())
}
