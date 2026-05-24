use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_deposit(_accounts: &[AccountInfo], _amount: u64, _min_shares_out: u64) -> ProgramResult {
    Ok(())
}
