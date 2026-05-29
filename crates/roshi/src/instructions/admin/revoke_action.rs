use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_revoke_action(_accounts: &[AccountInfo], _action_hash: [u8; 32]) -> ProgramResult {
    // TODO: close the authorized action PDA and revoke CPI permissions.
    Ok(())
}
