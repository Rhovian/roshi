use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_update_queue_authority(
    _accounts: &[AccountInfo],
    _queue_authority: [u8; 32],
) -> ProgramResult {
    Ok(())
}
