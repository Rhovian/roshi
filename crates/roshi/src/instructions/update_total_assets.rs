use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_update_total_assets(_accounts: &[AccountInfo], _external_assets: u64) -> ProgramResult {
    // TODO: read idle assets from the vault token account, add the trusted
    // external asset report, and enforce NAV update guardrails.
    let _ = _external_assets;
    Ok(())
}
