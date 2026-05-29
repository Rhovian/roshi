use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

/// Implements [`crate::instructions::RoshiInstruction::Deposit`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Depositor.
/// 1. `[writable]` Vault account receiving the deposit accounting update.
/// 2. `[writable]` User source token account for `asset_mint`.
/// 3. `[writable]` Vault custody token account for the selected asset.
/// 4. `[writable]` User share account or share accounting destination.
/// 5. `..` Optional Asset PDA and oracle accounts for non-base deposits.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation rejects
/// deposits while paused, routes base-mint deposits into custody owned by
/// `vault.deposit_sub_account`, normalizes enabled non-base assets through
/// their Asset PDA and oracle, mints or accounts shares, increases
/// `total_assets` and `total_shares`, and enforces `min_shares_out`.
pub fn try_deposit(
    _accounts: &[AccountInfo],
    _asset_mint: [u8; 32],
    _amount: u64,
    _min_shares_out: u64,
) -> ProgramResult {
    Ok(())
}
