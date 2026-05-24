use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_claim(_accounts: &[AccountInfo], _epoch: u64) -> ProgramResult {
    Ok(())
}
