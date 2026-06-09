//! On-chain `Vault` operations layered over the canonical
//! [`roshi_interface::state::Vault`].
//!
//! The struct and all its account-free logic live in the interface crate (shared
//! with off-chain readers — the single source of truth). The program adds the
//! operations that need account context (loading + PDA verification, role and
//! share-mint checks) through the [`VaultExt`] extension trait.

use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use roshi_interface::error::RoshiError;

use crate::state::Account;

pub use roshi_interface::state::{Role, Vault};

/// Account-context vault operations. The data and pure logic are inherent on
/// [`Vault`] in [`roshi_interface::state`]; these need an [`AccountInfo`].
pub trait VaultExt: Sized {
    /// Deserialize the vault from `account` and verify it is the canonical PDA.
    fn load_checked(account: &AccountInfo) -> Result<Self, ProgramError>;

    /// Verify `account` is the vault's share mint.
    fn verify_share_mint(&self, account: &AccountInfo) -> ProgramResult;

    /// Verify `signer` signed the transaction and holds `role`.
    fn verify_role(&self, role: Role, signer: &AccountInfo) -> ProgramResult;
}

impl VaultExt for Vault {
    // `Vault` is ~600 bytes; keep its deserialize + validation on this function's
    // own frame rather than inlining it into already-large instruction handlers
    // (which would blow SBF's 4 KiB per-frame stack limit, e.g. `try_swap`).
    #[inline(never)]
    fn load_checked(account: &AccountInfo) -> Result<Self, ProgramError> {
        let vault = Account::load_as::<Self>(account)?;
        vault.verify_address(account.key)?;
        Ok(vault)
    }

    fn verify_share_mint(&self, account: &AccountInfo) -> ProgramResult {
        if account.key != &Pubkey::from(self.share_mint) {
            return Err(RoshiError::InvalidMintAccount.into());
        }

        Ok(())
    }

    fn verify_role(&self, role: Role, signer: &AccountInfo) -> ProgramResult {
        if !signer.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        if !self.has_role(role, signer.key) {
            return Err(ProgramError::IllegalOwner);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::oracle::OracleConfig;
    use roshi_interface::state::VAULT_ACCOUNT_TAG;
    use wincode::{deserialize, serialize};

    fn new_test_vault() -> Vault {
        let base_mint = Pubkey::new_unique();
        let (_, bump) = Vault::find_address(b"test", &base_mint).unwrap();

        Vault::new(
            b"test",
            Pubkey::new_unique().to_bytes(),
            [2; 32],
            [3; 32],
            [4; 32],
            [5; 32],
            base_mint.to_bytes(),
            Pubkey::new_unique().to_bytes(),
            6,
            OracleConfig::default(),
            7,
            8,
            [9; 32],
            100,
            250,
            false,
            [0; 32],
            bump,
        )
        .unwrap()
    }

    #[test]
    fn serialized_vault_fits_allocated_account_space() {
        let vault = new_test_vault();
        let serialized = serialize(&Account::Vault(vault)).unwrap();

        assert!(serialized.len() <= Vault::SPACE);

        let mut account_data = vec![0; Vault::SPACE];
        account_data[..serialized.len()].copy_from_slice(&serialized);

        let decoded: Account = deserialize(&account_data).unwrap();
        let Account::Vault(decoded_vault) = decoded else {
            panic!("expected vault account");
        };
        assert_eq!(decoded_vault.tag_seed().unwrap(), b"test");
    }

    /// The program's `Account` enum tag and the off-chain reader
    /// (`Vault::from_account_data`) must agree on the wire layout.
    #[test]
    fn account_tag_agrees_with_off_chain_reader() {
        let vault = new_test_vault();
        let serialized = serialize(&Account::Vault(vault)).unwrap();

        assert_eq!(serialized[0], VAULT_ACCOUNT_TAG);
        assert_eq!(Vault::from_account_data(&serialized).unwrap(), vault);
    }

    #[test]
    fn load_checked_round_trips_and_verifies_pda() {
        let vault = new_test_vault();
        let key = Vault::find_address(b"test", &Pubkey::from(vault.base_mint))
            .unwrap()
            .0;
        let mut data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(Vault::load_checked(&account).unwrap(), vault);
    }

    #[test]
    fn load_checked_rejects_wrong_pda() {
        let vault = new_test_vault();
        let key = Pubkey::new_unique();
        let mut data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(
            Vault::load_checked(&account),
            Err(ProgramError::InvalidSeeds)
        );
    }
}
