use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::UpdateVaultConfigArgs;

pub fn try_update_vault_config(
    _accounts: &[AccountInfo],
    _args: UpdateVaultConfigArgs,
) -> ProgramResult {
    // TODO: verify vault admin and atomically replace mutable role, pause,
    // default subaccount, fee, and guardrail config fields.
    let _ = _args;
    Ok(())
}
