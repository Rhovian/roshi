use roshi_interface::{
    access::verify_access_merkle_proof, error::RoshiError, math::validate_percentage_bps,
    oracle::OracleConfig,
};
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

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
    pub tag: [u8; 32],
    pub tag_len: u8,
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
    pub const MAX_TAG_LEN: usize = 32;
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    pub fn new(
        tag: &[u8],
        admin: [u8; 32],
        strategist: [u8; 32],
        nav_authority: [u8; 32],
        withdrawal_authority: [u8; 32],
        base_mint: [u8; 32],
        share_mint: [u8; 32],
        base_decimals: u8,
        base_oracle: OracleConfig,
        deposit_sub_account: u8,
        withdraw_sub_account: u8,
        fee_collector: [u8; 32],
        performance_fee_bps: u16,
        withdrawal_buffer_bps: u16,
        max_change_bps: u16,
        min_update_interval: i64,
        private: bool,
        access_merkle_root: [u8; 32],
        bump: u8,
    ) -> Result<Self, ProgramError> {
        Self::validate_config(
            base_mint,
            share_mint,
            performance_fee_bps,
            withdrawal_buffer_bps,
            max_change_bps,
            min_update_interval,
        )?;

        let (tag, tag_len) = Self::pack_tag(tag)?;

        Ok(Self {
            tag,
            tag_len,
            admin,
            strategist,
            nav_authority,
            withdrawal_authority,
            base_mint,
            share_mint,
            base_decimals,
            base_oracle,
            deposit_sub_account,
            withdraw_sub_account,
            fee_collector,
            total_assets: 0,
            last_report_hash: [0; 32],
            total_shares: 0,
            pending_withdrawal_assets: 0,
            high_watermark: 0,
            performance_fee_bps,
            withdrawal_buffer_bps,
            max_change_bps,
            min_update_interval,
            last_update_ts: 0,
            current_withdrawal_epoch: 1,
            processed_withdrawal_epoch: 0,
            deposits_paused: false,
            withdrawals_paused: false,
            manage_paused: false,
            private,
            access_merkle_root,
            bump,
        })
    }

    pub fn validate_config(
        base_mint: [u8; 32],
        share_mint: [u8; 32],
        performance_fee_bps: u16,
        withdrawal_buffer_bps: u16,
        max_change_bps: u16,
        min_update_interval: i64,
    ) -> ProgramResult {
        validate_percentage_bps(performance_fee_bps)?;
        validate_percentage_bps(withdrawal_buffer_bps)?;
        validate_percentage_bps(max_change_bps)?;

        if min_update_interval < 0 || base_mint == share_mint {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(())
    }

    pub fn pack_tag(tag: &[u8]) -> Result<([u8; Self::MAX_TAG_LEN], u8), ProgramError> {
        Self::validate_tag(tag)?;

        let mut packed_tag = [0; Self::MAX_TAG_LEN];
        packed_tag[..tag.len()].copy_from_slice(tag);

        Ok((packed_tag, tag.len() as u8))
    }

    pub fn unpack_tag(tag: &[u8; Self::MAX_TAG_LEN], tag_len: u8) -> Result<&[u8], ProgramError> {
        let tag_len = usize::from(tag_len);
        let tag = tag
            .get(..tag_len)
            .ok_or(ProgramError::from(RoshiError::InvalidVaultTag))?;
        Self::validate_tag(tag)?;

        Ok(tag)
    }

    pub fn tag_seed(&self) -> Result<&[u8], ProgramError> {
        Self::unpack_tag(&self.tag, self.tag_len)
    }

    pub fn find_address(tag: &[u8], base_mint: &Pubkey) -> Result<(Pubkey, u8), ProgramError> {
        Self::validate_tag(tag)?;

        Ok(Pubkey::find_program_address(
            &[Self::SEED, tag, base_mint.as_ref()],
            &crate::ID,
        ))
    }

    fn validate_tag(tag: &[u8]) -> ProgramResult {
        if tag.is_empty() || tag.len() > Self::MAX_TAG_LEN {
            return Err(RoshiError::InvalidVaultTag.into());
        }

        Ok(())
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
        let base_mint = Pubkey::from(self.base_mint);
        let (expected_vault_key, expected_bump) = Self::find_address(self.tag_seed()?, &base_mint)?;

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
    use wincode::{deserialize, serialize};

    fn new_test_vault(private: bool, access_merkle_root: [u8; 32]) -> Vault {
        let admin = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let (_, bump) = Vault::find_address(b"test", &base_mint).unwrap();

        Vault::new(
            b"test",
            admin.to_bytes(),
            [2; 32],
            [3; 32],
            [4; 32],
            base_mint.to_bytes(),
            Pubkey::new_unique().to_bytes(),
            6,
            OracleConfig::default(),
            7,
            8,
            [9; 32],
            100,
            250,
            500,
            60,
            private,
            access_merkle_root,
            bump,
        )
        .unwrap()
    }

    fn test_vault(private: bool, access_merkle_root: [u8; 32]) -> Vault {
        new_test_vault(private, access_merkle_root)
    }

    #[test]
    fn new_initializes_default_accounting_and_config() {
        let vault = new_test_vault(true, [10; 32]);

        assert_eq!(vault.tag_seed().unwrap(), b"test");
        assert_eq!(vault.strategist, [2; 32]);
        assert_eq!(vault.nav_authority, [3; 32]);
        assert_eq!(vault.withdrawal_authority, [4; 32]);
        assert_eq!(vault.base_decimals, 6);
        assert_eq!(vault.deposit_sub_account, 7);
        assert_eq!(vault.withdraw_sub_account, 8);
        assert_eq!(vault.fee_collector, [9; 32]);
        assert_eq!(vault.total_assets, 0);
        assert_eq!(vault.total_shares, 0);
        assert_eq!(vault.pending_withdrawal_assets, 0);
        assert_eq!(vault.high_watermark, 0);
        assert_eq!(vault.performance_fee_bps, 100);
        assert_eq!(vault.withdrawal_buffer_bps, 250);
        assert_eq!(vault.max_change_bps, 500);
        assert_eq!(vault.min_update_interval, 60);
        assert_eq!(vault.last_update_ts, 0);
        assert_eq!(vault.current_withdrawal_epoch, 1);
        assert_eq!(vault.processed_withdrawal_epoch, 0);
        assert!(!vault.deposits_paused);
        assert!(!vault.withdrawals_paused);
        assert!(!vault.manage_paused);
        assert!(vault.private);
        assert_eq!(vault.access_merkle_root, [10; 32]);
    }

    #[test]
    fn serialized_vault_fits_allocated_account_space() {
        let vault = new_test_vault(false, [0; 32]);
        let serialized = serialize(&crate::state::Account::Vault(vault)).unwrap();

        assert!(serialized.len() <= Vault::SPACE);

        let mut account_data = vec![0; Vault::SPACE];
        account_data[..serialized.len()].copy_from_slice(&serialized);

        let decoded: crate::state::Account = deserialize(&account_data).unwrap();
        let crate::state::Account::Vault(decoded_vault) = decoded else {
            panic!("expected vault account");
        };
        assert_eq!(decoded_vault.tag_seed().unwrap(), b"test");
    }

    #[test]
    fn unpack_tag_rejects_invalid_tags() {
        let (tag, _) = Vault::pack_tag(b"test").unwrap();

        assert!(matches!(
            Vault::unpack_tag(&tag, 0),
            Err(error) if error == ProgramError::from(RoshiError::InvalidVaultTag)
        ));
        assert!(matches!(
            Vault::unpack_tag(&tag, 33),
            Err(error) if error == ProgramError::from(RoshiError::InvalidVaultTag)
        ));
    }

    #[test]
    fn validate_config_rejects_invalid_bps() {
        assert!(matches!(
            Vault::validate_config([1; 32], [2; 32], 10_001, 0, 0, 0),
            Err(error) if error == ProgramError::from(RoshiError::InvalidBps)
        ));
    }

    #[test]
    fn validate_config_rejects_negative_min_update_interval() {
        assert!(matches!(
            Vault::validate_config([1; 32], [2; 32], 0, 0, 0, -1),
            Err(ProgramError::InvalidArgument)
        ));
    }

    #[test]
    fn validate_config_rejects_matching_base_and_share_mints() {
        assert!(matches!(
            Vault::validate_config([1; 32], [1; 32], 0, 0, 0, 0),
            Err(ProgramError::InvalidArgument)
        ));
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
