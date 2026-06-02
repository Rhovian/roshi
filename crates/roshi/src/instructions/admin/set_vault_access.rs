use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_vault_as_admin, SetVaultAccessArgs};

/// Implements [`crate::instructions::RoshiInstructionTag::SetVaultAccess`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account whose access mode is updated.
///
/// Verifies the vault admin and atomically updates `private` and
/// `access_merkle_root` without touching role, fee, guardrail, pause, or
/// subaccount configuration.
pub fn try_set_vault_access(accounts: &[AccountInfo], args: SetVaultAccessArgs) -> ProgramResult {
    update_writable_vault_as_admin(accounts, |vault| {
        vault.set_private(args.private);
        vault.access_merkle_root = args.access_merkle_root;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::oracle::OracleConfig;
    use solana_program_error::ProgramError;
    use solana_pubkey::Pubkey;
    use wincode::{deserialize, serialize};

    use crate::state::{vault::Vault, Account};

    fn test_vault(admin: Pubkey, base_mint: Pubkey) -> (Pubkey, Vault) {
        let (vault_key, bump) = Vault::find_address(b"test", &base_mint).unwrap();
        let vault = Vault::new(
            b"test",
            admin.to_bytes(),
            admin.to_bytes(),
            admin.to_bytes(),
            admin.to_bytes(),
            base_mint.to_bytes(),
            Pubkey::new_unique().to_bytes(),
            6,
            OracleConfig::default(),
            0,
            0,
            admin.to_bytes(),
            0,
            0,
            0,
            false,
            [0; 32],
            bump,
        )
        .unwrap();

        (vault_key, vault)
    }

    fn load_vault(vault_account: &AccountInfo) -> Vault {
        let account = deserialize(&vault_account.data.borrow()).unwrap();
        let Account::Vault(vault) = account else {
            panic!("expected vault account");
        };

        vault
    }

    #[test]
    fn updates_vault_access_when_admin_signs() {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, vault) = test_vault(admin, base_mint);
        let mut admin_lamports = 1;
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let admin_account = AccountInfo::new(
            &admin,
            true,
            false,
            &mut admin_lamports,
            &mut [],
            &owner,
            false,
        );
        let vault_account = AccountInfo::new(
            &vault_key,
            false,
            true,
            &mut vault_lamports,
            &mut vault_data,
            &owner,
            false,
        );
        let root = [7; 32];

        try_set_vault_access(
            &[admin_account, vault_account.clone()],
            SetVaultAccessArgs {
                private: true,
                access_merkle_root: root,
            },
        )
        .unwrap();

        let vault = load_vault(&vault_account);

        assert_eq!(vault.private(), Ok(true));
        assert_eq!(vault.access_merkle_root, root);
    }

    #[test]
    fn rejects_non_admin_signer() {
        let admin = Pubkey::new_unique();
        let wrong_admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, vault) = test_vault(admin, base_mint);
        let mut admin_lamports = 1;
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let admin_account = AccountInfo::new(
            &wrong_admin,
            true,
            false,
            &mut admin_lamports,
            &mut [],
            &owner,
            false,
        );
        let vault_account = AccountInfo::new(
            &vault_key,
            false,
            true,
            &mut vault_lamports,
            &mut vault_data,
            &owner,
            false,
        );

        assert_eq!(
            try_set_vault_access(
                &[admin_account, vault_account],
                SetVaultAccessArgs {
                    private: true,
                    access_merkle_root: [7; 32],
                },
            ),
            Err(ProgramError::IllegalOwner)
        );
    }

    #[test]
    fn rejects_missing_admin_signature() {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, vault) = test_vault(admin, base_mint);
        let mut admin_lamports = 1;
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let admin_account = AccountInfo::new(
            &admin,
            false,
            false,
            &mut admin_lamports,
            &mut [],
            &owner,
            false,
        );
        let vault_account = AccountInfo::new(
            &vault_key,
            false,
            true,
            &mut vault_lamports,
            &mut vault_data,
            &owner,
            false,
        );

        assert_eq!(
            try_set_vault_access(
                &[admin_account, vault_account],
                SetVaultAccessArgs {
                    private: true,
                    access_merkle_root: [7; 32],
                },
            ),
            Err(ProgramError::MissingRequiredSignature)
        );
    }

    #[test]
    fn rejects_non_writable_vault_account() {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, vault) = test_vault(admin, base_mint);
        let mut admin_lamports = 1;
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let admin_account = AccountInfo::new(
            &admin,
            true,
            false,
            &mut admin_lamports,
            &mut [],
            &owner,
            false,
        );
        let vault_account = AccountInfo::new(
            &vault_key,
            false,
            false,
            &mut vault_lamports,
            &mut vault_data,
            &owner,
            false,
        );

        assert_eq!(
            try_set_vault_access(
                &[admin_account, vault_account],
                SetVaultAccessArgs {
                    private: true,
                    access_merkle_root: [7; 32],
                },
            ),
            Err(ProgramError::InvalidAccountData)
        );
    }

    #[test]
    fn flips_private_vault_back_to_public() {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, mut vault) = test_vault(admin, base_mint);
        vault.set_private(true);
        vault.access_merkle_root = [7; 32];
        let mut admin_lamports = 1;
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let admin_account = AccountInfo::new(
            &admin,
            true,
            false,
            &mut admin_lamports,
            &mut [],
            &owner,
            false,
        );
        let vault_account = AccountInfo::new(
            &vault_key,
            false,
            true,
            &mut vault_lamports,
            &mut vault_data,
            &owner,
            false,
        );

        try_set_vault_access(
            &[admin_account, vault_account.clone()],
            SetVaultAccessArgs {
                private: false,
                access_merkle_root: [0; 32],
            },
        )
        .unwrap();

        let vault = load_vault(&vault_account);

        assert_eq!(vault.private(), Ok(false));
        assert_eq!(vault.access_merkle_root, [0; 32]);
    }

    #[test]
    fn rotates_access_root_while_private() {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, mut vault) = test_vault(admin, base_mint);
        vault.set_private(true);
        vault.access_merkle_root = [7; 32];
        let mut admin_lamports = 1;
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let owner = crate::ID;
        let admin_account = AccountInfo::new(
            &admin,
            true,
            false,
            &mut admin_lamports,
            &mut [],
            &owner,
            false,
        );
        let vault_account = AccountInfo::new(
            &vault_key,
            false,
            true,
            &mut vault_lamports,
            &mut vault_data,
            &owner,
            false,
        );
        let new_root = [8; 32];

        try_set_vault_access(
            &[admin_account, vault_account.clone()],
            SetVaultAccessArgs {
                private: true,
                access_merkle_root: new_root,
            },
        )
        .unwrap();

        let vault = load_vault(&vault_account);

        assert_eq!(vault.private(), Ok(true));
        assert_eq!(vault.access_merkle_root, new_root);
    }
}
