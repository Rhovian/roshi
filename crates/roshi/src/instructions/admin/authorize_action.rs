use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::state::action::Ops;

pub fn try_authorize_action(
    _accounts: &[AccountInfo],
    _action_hash: [u8; 32],
    _ops: Ops,
) -> ProgramResult {
    // TODO: implement action PDA creation and authorization flow.
    let _ = _ops;
    Ok(())
}
