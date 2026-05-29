use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

/// Implements [`crate::instructions::RoshiInstruction::UpdateTotalAssets`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault NAV authority.
/// 1. `[writable]` Vault account receiving the accepted NAV report.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// NAV authority, enforces `min_update_interval` and `max_change_bps`, stores
/// the accepted total NAV and report commitment, and updates the report
/// timestamp. Token balances remain settlement-liquidity checks, not NAV truth.
pub fn try_update_total_assets(
    _accounts: &[AccountInfo],
    _total_assets: u64,
    _report_hash: [u8; 32],
) -> ProgramResult {
    let _ = (_total_assets, _report_hash);
    Ok(())
}
