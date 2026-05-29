use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_process_withdrawals(_accounts: &[AccountInfo]) -> ProgramResult {
    // TODO: verify withdrawal_authority, validate one or more queued withdrawal
    // tickets, transfer owed base assets from withdraw subaccount custody to
    // each ticket owner, and close or clear settled ticket slots.
    // The strategist is responsible for returning liquidity before this
    // instruction is called; this instruction performs the settlement.
    Ok(())
}
