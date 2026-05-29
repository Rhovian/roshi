use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::UpdateAssetArgs;

pub fn try_update_asset(_accounts: &[AccountInfo], _args: UpdateAssetArgs) -> ProgramResult {
    // TODO: verify vault admin, load the supported non-base Asset PDA, and
    // atomically replace mutable custody/oracle/deposit-limit fields.
    Ok(())
}
