use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::InitializeVaultArgs;

pub fn try_initialize_vault(
    _accounts: &[AccountInfo],
    _args: InitializeVaultArgs,
) -> ProgramResult {
    // TODO: implement vault creation, role config, pause defaults, default
    // subaccounts, and initial accounting state setup.
    let _ = _args;
    Ok(())
}
