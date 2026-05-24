use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_update_total_assets(
    _accounts: &[AccountInfo],
    _total_assets: u64,
    _external_assets: u64,
) -> ProgramResult {
    // TODO: record trusted NAV updates and enforce update guardrails.
    let _ = _external_assets;
    let _ = _total_assets;
    Ok(())
}
