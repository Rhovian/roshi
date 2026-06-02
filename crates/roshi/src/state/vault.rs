use roshi_interface::{
    access::verify_access_merkle_proof, error::RoshiError, math::validate_percentage_bps,
    oracle::OracleConfig,
};
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

use crate::state::flags;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin,
    Strategist,
    NavAuthority,
    WithdrawalAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct Vault {
    pub base_oracle: OracleConfig,
    pub total_assets: u64,
    pub pending_withdrawal_assets: u64,
    pub fees_payable: u64,
    pub high_watermark: u64,
    pub report_epoch: u64,
    pub requested_withdrawal_shares: u64,
    pub last_update_ts: i64,
    pub tag: [u8; 32],
    pub admin: [u8; 32],
    pub strategist: [u8; 32],
    pub nav_authority: [u8; 32],
    pub withdrawal_authority: [u8; 32],
    pub base_mint: [u8; 32],
    pub share_mint: [u8; 32],
    pub fee_collector: [u8; 32],
    pub last_report_hash: [u8; 32],
    pub access_merkle_root: [u8; 32],
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub tag_len: u8,
    pub base_decimals: u8,
    pub deposit_sub_account: u8,
    pub withdraw_sub_account: u8,
    deposits_paused_flag: u8,
    withdrawals_paused_flag: u8,
    manage_paused_flag: u8,
    private_flag: u8,
    pub bump: u8,
    _padding: [u8; 3],
}

impl Vault {
    pub const SEED: &'static [u8] = b"vault";
    pub const MAX_TAG_LEN: usize = 32;
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    #[allow(clippy::too_many_arguments)]
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
        private: bool,
        access_merkle_root: [u8; 32],
        bump: u8,
    ) -> Result<Self, ProgramError> {
        Self::validate_config(
            base_mint,
            share_mint,
            performance_fee_bps,
            withdrawal_buffer_bps,
        )?;
        base_oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidVaultState))?;

        let (tag, tag_len) = Self::pack_tag(tag)?;

        Ok(Self {
            base_oracle,
            total_assets: 0,
            pending_withdrawal_assets: 0,
            fees_payable: 0,
            high_watermark: 0,
            report_epoch: 0,
            requested_withdrawal_shares: 0,
            last_update_ts: 0,
            tag,
            admin,
            strategist,
            nav_authority,
            withdrawal_authority,
            base_mint,
            share_mint,
            fee_collector,
            last_report_hash: [0; 32],
            access_merkle_root,
            performance_fee_bps,
            withdrawal_buffer_bps,
            tag_len,
            base_decimals,
            deposit_sub_account,
            withdraw_sub_account,
            deposits_paused_flag: flags::bool_to_flag(false),
            withdrawals_paused_flag: flags::bool_to_flag(false),
            manage_paused_flag: flags::bool_to_flag(false),
            private_flag: flags::bool_to_flag(private),
            bump,
            _padding: [0; 3],
        })
    }

    pub fn validate_config(
        base_mint: [u8; 32],
        share_mint: [u8; 32],
        performance_fee_bps: u16,
        withdrawal_buffer_bps: u16,
    ) -> ProgramResult {
        validate_percentage_bps(performance_fee_bps)?;
        validate_percentage_bps(withdrawal_buffer_bps)?;

        if base_mint == share_mint {
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
        if self.manage_paused()? {
            return Err(RoshiError::VaultPaused.into());
        }

        Ok(())
    }

    pub fn allows_depositor(&self, depositor: &Pubkey, proof: &[[u8; 32]]) -> bool {
        match self.private() {
            Ok(false) => true,
            Ok(true) => verify_access_merkle_proof(depositor, &self.access_merkle_root, proof),
            Err(_) => false,
        }
    }

    pub fn deposits_paused(&self) -> Result<bool, ProgramError> {
        flags::flag_to_bool(self.deposits_paused_flag, RoshiError::InvalidVaultState)
    }

    pub fn withdrawals_paused(&self) -> Result<bool, ProgramError> {
        flags::flag_to_bool(self.withdrawals_paused_flag, RoshiError::InvalidVaultState)
    }

    pub fn manage_paused(&self) -> Result<bool, ProgramError> {
        flags::flag_to_bool(self.manage_paused_flag, RoshiError::InvalidVaultState)
    }

    pub fn private(&self) -> Result<bool, ProgramError> {
        flags::flag_to_bool(self.private_flag, RoshiError::InvalidVaultState)
    }

    pub fn set_deposits_paused(&mut self, deposits_paused: bool) {
        self.deposits_paused_flag = flags::bool_to_flag(deposits_paused);
    }

    pub fn set_withdrawals_paused(&mut self, withdrawals_paused: bool) {
        self.withdrawals_paused_flag = flags::bool_to_flag(withdrawals_paused);
    }

    pub fn set_manage_paused(&mut self, manage_paused: bool) {
        self.manage_paused_flag = flags::bool_to_flag(manage_paused);
    }

    pub fn set_private(&mut self, private: bool) {
        self.private_flag = flags::bool_to_flag(private);
    }

    pub fn validate_state(&self) -> ProgramResult {
        Self::unpack_tag(&self.tag, self.tag_len)?;
        Self::validate_config(
            self.base_mint,
            self.share_mint,
            self.performance_fee_bps,
            self.withdrawal_buffer_bps,
        )?;
        self.base_oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidVaultState))?;
        flags::validate_flag(self.deposits_paused_flag, RoshiError::InvalidVaultState)?;
        flags::validate_flag(self.withdrawals_paused_flag, RoshiError::InvalidVaultState)?;
        flags::validate_flag(self.manage_paused_flag, RoshiError::InvalidVaultState)?;
        flags::validate_flag(self.private_flag, RoshiError::InvalidVaultState)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::access::{access_merkle_leaf, access_merkle_node};
    use wincode::{
        config::DefaultConfig, deserialize, serialize, SchemaRead, SchemaWrite, TypeMeta,
    };

    use crate::state::Account;

    fn assert_zero_copy<T>()
    where
        T: wincode::ZeroCopy,
        T: for<'de> SchemaRead<'de, DefaultConfig> + SchemaWrite<DefaultConfig>,
    {
        assert_eq!(
            <T as SchemaRead<'_, DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
        assert_eq!(
            <T as SchemaWrite<DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
    }

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
        assert_eq!(vault.pending_withdrawal_assets, 0);
        assert_eq!(vault.fees_payable, 0);
        assert_eq!(vault.high_watermark, 0);
        assert_eq!(vault.report_epoch, 0);
        assert_eq!(vault.requested_withdrawal_shares, 0);
        assert_eq!(vault.performance_fee_bps, 100);
        assert_eq!(vault.withdrawal_buffer_bps, 250);
        assert_eq!(vault.last_update_ts, 0);
        assert_eq!(vault.deposits_paused(), Ok(false));
        assert_eq!(vault.withdrawals_paused(), Ok(false));
        assert_eq!(vault.manage_paused(), Ok(false));
        assert_eq!(vault.private(), Ok(true));
        assert_eq!(vault.access_merkle_root, [10; 32]);
    }

    #[test]
    fn serialized_vault_fits_allocated_account_space() {
        let vault = new_test_vault(false, [0; 32]);
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

    #[test]
    fn vault_is_zero_copy_with_explicit_padding() {
        let vault = new_test_vault(false, [0; 32]);

        assert_zero_copy::<Vault>();
        assert_eq!(core::mem::size_of::<Vault>(), 560);
        assert_eq!(Vault::SPACE, 561);
        assert_eq!(
            serialize(&vault).unwrap().len(),
            core::mem::size_of::<Vault>()
        );
    }

    #[test]
    fn pause_and_access_flags_use_typed_accessors() {
        let mut vault = new_test_vault(false, [0; 32]);

        assert_eq!(vault.deposits_paused(), Ok(false));
        assert_eq!(vault.withdrawals_paused(), Ok(false));
        assert_eq!(vault.manage_paused(), Ok(false));
        assert_eq!(vault.private(), Ok(false));

        vault.set_deposits_paused(true);
        vault.set_withdrawals_paused(true);
        vault.set_manage_paused(true);
        vault.set_private(true);

        assert_eq!(vault.deposits_paused(), Ok(true));
        assert_eq!(vault.withdrawals_paused(), Ok(true));
        assert_eq!(vault.manage_paused(), Ok(true));
        assert_eq!(vault.private(), Ok(true));
    }

    #[test]
    fn load_as_rejects_invalid_vault_flags() {
        let mut vault = new_test_vault(false, [0; 32]);
        vault.manage_paused_flag = 255;
        let mut data = serialize(&Account::Vault(vault)).unwrap();
        let key = Pubkey::new_unique();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(
            Account::load_as::<Vault>(&account),
            Err(ProgramError::from(RoshiError::InvalidVaultState))
        );
    }

    #[test]
    fn load_as_rejects_invalid_base_oracle_kind() {
        let vault = new_test_vault(false, [0; 32]);
        let mut data = serialize(&Account::Vault(vault)).unwrap();
        let oracle_kind_offset = 1
            + core::mem::size_of::<roshi_interface::oracle::SwitchboardOracleConfig>()
            + core::mem::size_of::<roshi_interface::oracle::PythOracleConfig>();
        data[oracle_kind_offset] = 255;
        let key = Pubkey::new_unique();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(
            Account::load_as::<Vault>(&account),
            Err(ProgramError::from(RoshiError::InvalidVaultState))
        );
    }

    #[test]
    fn verify_manage_enabled_rejects_paused_vault() {
        let mut vault = new_test_vault(false, [0; 32]);

        vault.set_manage_paused(true);

        assert_eq!(
            vault.verify_manage_enabled(),
            Err(ProgramError::from(RoshiError::VaultPaused))
        );
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
            Vault::validate_config([1; 32], [2; 32], 10_001, 0),
            Err(error) if error == ProgramError::from(RoshiError::InvalidBps)
        ));
    }

    #[test]
    fn validate_config_rejects_matching_base_and_share_mints() {
        assert!(matches!(
            Vault::validate_config([1; 32], [1; 32], 0, 0),
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
