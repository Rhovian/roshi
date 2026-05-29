use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_deposit(
    _accounts: &[AccountInfo],
    _asset_mint: [u8; 32],
    _amount: u64,
    _min_shares_out: u64,
) -> ProgramResult {
    // TODO: base-mint deposits use custody owned by vault.deposit_sub_account.
    // Non-base deposits must load the Asset PDA for (vault, asset_mint), use
    // its custody account and oracle config, and normalize into base units
    // before minting shares.
    Ok(())
}
