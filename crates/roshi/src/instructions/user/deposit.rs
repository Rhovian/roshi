use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use roshi_interface::{access::MAX_ACCESS_PROOF_LEN, error::RoshiError};

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
/// If the vault is private, instruction data must include a Merkle proof for
/// the depositor's wallet against `vault.access_merkle_root`.
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
    access_proof: Vec<[u8; 32]>,
) -> ProgramResult {
    if access_proof.len() > MAX_ACCESS_PROOF_LEN {
        return Err(RoshiError::InvalidAccessProof.into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program_error::ProgramError;

    #[test]
    fn rejects_oversized_access_proof() {
        assert_eq!(
            try_deposit(&[], [0; 32], 1, 1, vec![[0; 32]; MAX_ACCESS_PROOF_LEN + 1],),
            Err(ProgramError::from(RoshiError::InvalidAccessProof))
        );
    }
}
