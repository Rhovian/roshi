use roshi_interface::{access::verify_access_merkle_proof, oracle::OracleConfig};
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

use crate::error::RoshiError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin,
    Strategist,
    NavAuthority,
    WithdrawalAuthority,
}

#[derive(SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct Vault {
    pub admin: [u8; 32],
    pub strategist: [u8; 32],
    pub nav_authority: [u8; 32],
    pub withdrawal_authority: [u8; 32],
    pub base_mint: [u8; 32],
    pub share_mint: [u8; 32],
    pub base_decimals: u8,
    pub base_oracle: OracleConfig,
    pub deposit_sub_account: u8,
    pub withdraw_sub_account: u8,
    pub fee_collector: [u8; 32],
    pub total_assets: u64,
    pub last_report_hash: [u8; 32],
    pub total_shares: u64,
    pub pending_withdrawal_assets: u64,
    pub high_watermark: u64,
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub max_change_bps: u16,
    pub min_update_interval: i64,
    pub last_update_ts: i64,
    pub current_withdrawal_epoch: u64,
    pub processed_withdrawal_epoch: u64,
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
    pub manage_paused: bool,
    pub private: bool,
    pub access_merkle_root: [u8; 32],
    pub bump: u8,
}

impl Vault {
    pub const SEED: &'static [u8] = b"vault";

    pub fn find_address(admin: &Pubkey, base_mint: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED, admin.as_ref(), base_mint.as_ref()],
            &crate::ID,
        )
    }

    pub fn authority_for_role(&self, role: Role) -> Pubkey {
        match role {
            Role::Admin => Pubkey::from(self.admin),
            Role::Strategist => Pubkey::from(self.strategist),
            Role::NavAuthority => Pubkey::from(self.nav_authority),
            Role::WithdrawalAuthority => Pubkey::from(self.withdrawal_authority),
        }
    }

    pub fn has_role(&self, role: Role, signer: &Pubkey) -> bool {
        self.authority_for_role(role) == *signer
    }

    pub fn verify_address(&self, vault_key: &Pubkey) -> ProgramResult {
        let admin = Pubkey::from(self.admin);
        let base_mint = Pubkey::from(self.base_mint);
        let (expected_vault_key, expected_bump) = Self::find_address(&admin, &base_mint);

        if vault_key != &expected_vault_key || self.bump != expected_bump {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(())
    }

    pub fn verify_role(&self, role: Role, signer: &AccountInfo) -> ProgramResult {
        if !signer.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        if !self.has_role(role, signer.key) {
            return Err(ProgramError::IllegalOwner);
        }

        Ok(())
    }

    pub fn verify_manage_enabled(&self) -> ProgramResult {
        if self.manage_paused {
            return Err(RoshiError::VaultPaused.into());
        }

        Ok(())
    }

    pub fn allows_depositor(&self, depositor: &Pubkey, proof: &[[u8; 32]]) -> bool {
        !self.private || verify_access_merkle_proof(depositor, &self.access_merkle_root, proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::access::{access_merkle_leaf, access_merkle_node};

    fn test_vault(private: bool, access_merkle_root: [u8; 32]) -> Vault {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (_, bump) = Vault::find_address(&admin, &base_mint);

        Vault {
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
            private,
            access_merkle_root,
            bump,
        }
    }

    #[test]
    fn public_vault_allows_any_depositor_without_proof() {
        let vault = test_vault(false, [0; 32]);

        assert!(vault.allows_depositor(&Pubkey::new_unique(), &[]));
        assert!(vault.allows_depositor(&Pubkey::new_unique(), &[[7; 32]]));
    }

    #[test]
    fn private_vault_accepts_valid_access_proof() {
        let allowed = Pubkey::new_unique();
        let sibling = access_merkle_leaf(&Pubkey::new_unique());
        let root = access_merkle_node(&access_merkle_leaf(&allowed), &sibling);
        let vault = test_vault(true, root);

        assert!(vault.allows_depositor(&allowed, &[sibling]));
    }

    #[test]
    fn private_vault_rejects_missing_or_wrong_access_proof() {
        let allowed = Pubkey::new_unique();
        let sibling = access_merkle_leaf(&Pubkey::new_unique());
        let root = access_merkle_node(&access_merkle_leaf(&allowed), &sibling);
        let vault = test_vault(true, root);

        assert!(!vault.allows_depositor(&allowed, &[]));
        assert!(!vault.allows_depositor(&Pubkey::new_unique(), &[sibling]));
        assert!(!vault.allows_depositor(&allowed, &[[9; 32]]));
    }
}
