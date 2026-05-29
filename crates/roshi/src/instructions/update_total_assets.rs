use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_update_total_assets(
    _accounts: &[AccountInfo],
    _total_assets: u64,
    _report_hash: [u8; 32],
) -> ProgramResult {
    // TODO: verify nav_authority, enforce NAV update guardrails, then store
    // the accepted total NAV and report commitment. Token balances are used for
    // settlement liquidity checks, not NAV truth.
    let _ = (_total_assets, _report_hash);
    Ok(())
}
