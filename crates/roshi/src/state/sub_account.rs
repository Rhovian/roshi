use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

/// PDA signer namespace for vault custody and strategy execution.
///
/// Subaccounts are intentionally not Roshi-owned data accounts. They are PDA
/// authorities that can own token accounts and sign authorized CPIs.
pub struct VaultSubAccount;

impl VaultSubAccount {
    pub const SEED: &'static [u8] = b"sub_account";

    pub fn find_address(vault: &Pubkey, index: u8) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED, vault.as_ref(), &[index]], &crate::ID)
    }

    pub fn verify_account(
        vault: &Pubkey,
        index: u8,
        sub_account: &AccountInfo,
    ) -> Result<u8, ProgramError> {
        let (expected_sub_account_key, sub_account_bump) = Self::find_address(vault, index);
        if sub_account.key != &expected_sub_account_key {
            return Err(ProgramError::InvalidSeeds);
        }

        if sub_account.owner.to_bytes() != system_program::ID.to_bytes() {
            return Err(ProgramError::IllegalOwner);
        }

        if !sub_account.data_is_empty() || sub_account.executable {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(sub_account_bump)
    }
}
