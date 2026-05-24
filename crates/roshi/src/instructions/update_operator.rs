use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_update_operator(_accounts: &[AccountInfo], _operator: [u8; 32]) -> ProgramResult {
    Ok(())
}
