//! Program-side checks for the interface `Vault` account.

use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use roshi_interface::error::RoshiError;

use crate::state::Account;

pub use roshi_interface::state::{Role, Vault, VaultControls};

// `Vault` is ~600 bytes; keep its deserialize + validation on this function's
// own frame rather than inlining it into already-large instruction handlers.
#[inline(never)]
pub fn load_checked(account: &AccountInfo) -> Result<Vault, ProgramError> {
    let vault = Account::load_as::<Vault>(account)?;
    vault.verify_address(account.key)?;
    Ok(vault)
}

pub fn verify_share_mint(vault: &Vault, account: &AccountInfo) -> ProgramResult {
    if account.key != &Pubkey::from(vault.share_mint) {
        return Err(RoshiError::InvalidMintAccount.into());
    }

    Ok(())
}

pub fn verify_role(vault: &Vault, role: Role, signer: &AccountInfo) -> ProgramResult {
    if !signer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if !vault.has_role(role, signer.key) {
        return Err(ProgramError::IllegalOwner);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::oracle::OracleConfig;
    use roshi_interface::state::{VaultControls, VAULT_ACCOUNT_TAG};
    use wincode::{deserialize, serialize};

    fn new_test_vault() -> Vault {
        let base_mint = Pubkey::new_unique();
        let (_, bump) = Vault::find_address(b"test", &base_mint).unwrap();

        Vault::new(
            b"test",
            Pubkey::new_unique().to_bytes(),
            [2; 32],
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
            VaultControls::default(),
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

        assert_eq!(load_checked(&account).unwrap(), vault);
    }

    #[test]
    fn load_checked_rejects_wrong_pda() {
        let vault = new_test_vault();
        let key = Pubkey::new_unique();
        let mut data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(load_checked(&account), Err(ProgramError::InvalidSeeds));
    }
}
