use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{admin::vault_update::update_vault_as_admin, SetVaultAccessArgs};

/// Implements [`crate::instructions::RoshiInstruction::SetVaultAccess`].
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
    update_vault_as_admin(accounts, |vault| {
        vault.private = args.private;
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
        let (tag, tag_len) = Vault::pack_tag(b"test").unwrap();
        let (vault_key, bump) = Vault::find_address(b"test", &base_mint).unwrap();
        let vault = Vault {
            tag,
            tag_len,
            admin: admin.to_bytes(),
            strategist: admin.to_bytes(),
            nav_authority: admin.to_bytes(),
            withdrawal_authority: admin.to_bytes(),
            base_mint: base_mint.to_bytes(),
            share_mint: Pubkey::new_unique().to_bytes(),
            base_decimals: 6,
            base_oracle: OracleConfig::default(),
            deposit_sub_account: 0,
            withdraw_sub_account: 0,
            fee_collector: admin.to_bytes(),
            total_assets: 0,
            last_report_hash: [0; 32],
            total_shares: 0,
            pending_withdrawal_assets: 0,
            high_watermark: 0,
            performance_fee_bps: 0,
            withdrawal_buffer_bps: 0,
            max_change_bps: 0,
            min_update_interval: 0,
            last_update_ts: 0,
            current_withdrawal_epoch: 1,
            processed_withdrawal_epoch: 0,
            deposits_paused: false,
            withdrawals_paused: false,
            manage_paused: false,
            private: false,
            access_merkle_root: [0; 32],
            bump,
        };

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

        assert!(vault.private);
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
        vault.private = true;
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

        assert!(!vault.private);
        assert_eq!(vault.access_merkle_root, [0; 32]);
    }

    #[test]
    fn rotates_access_root_while_private() {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (vault_key, mut vault) = test_vault(admin, base_mint);
        vault.private = true;
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

        assert!(vault.private);
        assert_eq!(vault.access_merkle_root, new_root);
    }
}
