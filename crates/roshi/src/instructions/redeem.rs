use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_redeem(_accounts: &[AccountInfo], _shares: u64, _min_assets_out: u64) -> ProgramResult {
    Ok(())
}
