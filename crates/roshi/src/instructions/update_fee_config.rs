use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_update_fee_config(
    _accounts: &[AccountInfo],
    _performance_fee_bps: u16,
    _fee_collector: [u8; 32],
) -> ProgramResult {
    Ok(())
}
