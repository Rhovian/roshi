use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_vault_as_admin, TransferVaultAuthorityArgs};

/// Implements [`crate::instructions::RoshiInstruction::TransferVaultAuthority`].
///
/// # Accounts
///
/// 0. `[signer]` Current vault authority/admin.
/// 1. `[writable]` Vault account whose authority is transferred.
///
/// Verifies the current vault admin and replaces `vault.admin` with
/// `new_authority`. The vault PDA is derived from the vault tag and base asset,
/// so the vault address continues to verify after the admin changes.
pub fn try_transfer_vault_authority(
    accounts: &[AccountInfo],
    args: TransferVaultAuthorityArgs,
) -> ProgramResult {
    update_writable_vault_as_admin(accounts, |vault| {
        vault.admin = args.new_authority;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::oracle::OracleConfig;
    use roshi_interface::state::VaultControls;
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
            VaultControls::default(),
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
    fn transfers_vault_authority_without_changing_vault_address() {
        let admin = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, vault) = test_vault(admin, base_mint);
        let owner = crate::ID;
        let mut admin_lamports = 1;
        let mut admin_data = [];
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let admin_account = AccountInfo::new(
            &admin,
            true,
            false,
            &mut admin_lamports,
            &mut admin_data,
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

        try_transfer_vault_authority(
            &[admin_account, vault_account.clone()],
            TransferVaultAuthorityArgs {
                new_authority: new_authority.to_bytes(),
            },
        )
        .unwrap();

        let vault = load_vault(&vault_account);
        assert_eq!(vault.admin, new_authority.to_bytes());
        vault.verify_address(&vault_key).unwrap();
    }

    #[test]
    fn rejects_old_authority_after_transfer() {
        let old_authority = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, mut vault) = test_vault(old_authority, base_mint);
        vault.admin = new_authority.to_bytes();
        let owner = crate::ID;
        let mut authority_lamports = 1;
        let mut authority_data = [];
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let authority_account = AccountInfo::new(
            &old_authority,
            true,
            false,
            &mut authority_lamports,
            &mut authority_data,
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
            try_transfer_vault_authority(
                &[authority_account, vault_account],
                TransferVaultAuthorityArgs {
                    new_authority: Pubkey::new_unique().to_bytes(),
                },
            ),
            Err(ProgramError::IllegalOwner)
        );
    }

    #[test]
    fn rejects_missing_authority_signature() {
        let admin = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, vault) = test_vault(admin, base_mint);
        let owner = crate::ID;
        let mut admin_lamports = 1;
        let mut admin_data = [];
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let admin_account = AccountInfo::new(
            &admin,
            false,
            false,
            &mut admin_lamports,
            &mut admin_data,
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
            try_transfer_vault_authority(
                &[admin_account, vault_account],
                TransferVaultAuthorityArgs {
                    new_authority: new_authority.to_bytes(),
                },
            ),
            Err(ProgramError::MissingRequiredSignature)
        );
    }

    #[test]
    fn rejects_non_writable_vault_account() {
        let admin = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, vault) = test_vault(admin, base_mint);
        let owner = crate::ID;
        let mut admin_lamports = 1;
        let mut admin_data = [];
        let mut vault_lamports = 1;
        let mut vault_data = serialize(&Account::Vault(vault)).unwrap();
        let admin_account = AccountInfo::new(
            &admin,
            true,
            false,
            &mut admin_lamports,
            &mut admin_data,
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
            try_transfer_vault_authority(
                &[admin_account, vault_account],
                TransferVaultAuthorityArgs {
                    new_authority: new_authority.to_bytes(),
                },
            ),
            Err(ProgramError::InvalidAccountData)
        );
    }
}
