use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::SetPauseFlagsArgs;

pub fn try_set_pause_flags(_accounts: &[AccountInfo], _args: SetPauseFlagsArgs) -> ProgramResult {
    // TODO: verify vault admin and atomically update the vault pause flags.
    // Account layout: [admin, vault].
    let _ = _args;
    Ok(())
}
