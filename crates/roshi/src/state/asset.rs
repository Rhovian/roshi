use roshi_interface::{error::RoshiError, oracle::OracleConfig};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

const FLAG_FALSE: u8 = 0;
const FLAG_TRUE: u8 = 1;

const fn flag(value: bool) -> u8 {
    value as u8
}

fn bool_flag(flag: u8) -> Result<bool, ProgramError> {
    match flag {
        FLAG_FALSE => Ok(false),
        FLAG_TRUE => Ok(true),
        _ => Err(RoshiError::InvalidAssetAccount.into()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct Asset {
    /// Vault this non-base asset belongs to.
    pub vault: [u8; 32],
    /// Mint of the supported non-base deposit asset.
    pub asset_mint: [u8; 32],
    /// Oracle pricing one whole asset token. Direct mode: in whole base
    /// tokens. Routed mode: in the quote currency shared with the vault's
    /// `base_oracle`.
    pub oracle: OracleConfig,
    /// Inventory cap in asset atoms: deposits reject once the custody balance
    /// plus the deposit would exceed it. `u64::MAX` = uncapped (explicit — no
    /// zero-means-off magic; a zero cap blocks all deposits of this asset).
    pub deposit_cap_atoms: u64,
    /// Asset mint decimals.
    pub asset_decimals: u8,
    /// Whether deposits for this asset are enabled.
    enabled_flag: u8,
    /// Whether deposit pricing routes through the vault's `base_oracle`
    /// (asset/quote ÷ base/quote) instead of reading `oracle` as a direct
    /// asset/base feed.
    routed_flag: u8,
    pub bump: u8,
    _padding: [u8; 4],
}

impl Asset {
    pub const SEED: &'static [u8] = b"asset";
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vault: [u8; 32],
        asset_mint: [u8; 32],
        oracle: OracleConfig,
        asset_decimals: u8,
        enabled: bool,
        routed: bool,
        deposit_cap_atoms: u64,
        bump: u8,
    ) -> Result<Self, ProgramError> {
        oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidAssetAccount))?;

        Ok(Self {
            vault,
            asset_mint,
            oracle,
            deposit_cap_atoms,
            asset_decimals,
            enabled_flag: flag(enabled),
            routed_flag: flag(routed),
            bump,
            _padding: [0; 4],
        })
    }

    pub fn find_address(vault: &Pubkey, asset_mint: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED, vault.as_ref(), asset_mint.as_ref()],
            &crate::ID,
        )
    }

    pub fn enabled(&self) -> Result<bool, ProgramError> {
        bool_flag(self.enabled_flag)
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled_flag = flag(enabled);
    }

    pub fn routed(&self) -> Result<bool, ProgramError> {
        bool_flag(self.routed_flag)
    }

    pub fn set_routed(&mut self, routed: bool) {
        self.routed_flag = flag(routed);
    }

    pub fn validate_state(&self) -> ProgramResult {
        self.oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidAssetAccount))?;
        bool_flag(self.enabled_flag)?;
        bool_flag(self.routed_flag)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_account_info::AccountInfo;
    use wincode::{config::DefaultConfig, serialize, SchemaRead, SchemaWrite, TypeMeta};

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

    fn test_asset(enabled: bool) -> Asset {
        Asset::new(
            [1; 32],
            [2; 32],
            OracleConfig::default(),
            6,
            enabled,
            false,
            u64::MAX,
            7,
        )
        .unwrap()
    }

    #[test]
    fn asset_is_zero_copy_with_explicit_padding() {
        let asset = test_asset(true);

        assert_zero_copy::<Asset>();
        assert_eq!(core::mem::size_of::<Asset>(), 280);
        assert_eq!(Asset::SPACE, 281);
        assert_eq!(
            serialize(&asset).unwrap().len(),
            core::mem::size_of::<Asset>()
        );
    }

    #[test]
    fn enabled_and_routed_flags_use_typed_accessors() {
        let mut asset = test_asset(false);

        assert_eq!(asset.enabled(), Ok(false));
        asset.set_enabled(true);
        assert_eq!(asset.enabled(), Ok(true));

        assert_eq!(asset.routed(), Ok(false));
        asset.set_routed(true);
        assert_eq!(asset.routed(), Ok(true));
    }

    #[test]
    fn load_as_rejects_invalid_routed_flag() {
        let mut asset = test_asset(true);
        asset.routed_flag = 255;
        let mut data = serialize(&Account::Asset(asset)).unwrap();
        let key = Pubkey::new_unique();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(
            Account::load_as::<Asset>(&account),
            Err(ProgramError::from(RoshiError::InvalidAssetAccount))
        );
    }

    #[test]
    fn load_as_rejects_invalid_enabled_flag() {
        let mut asset = test_asset(true);
        asset.enabled_flag = 255;
        let mut data = serialize(&Account::Asset(asset)).unwrap();
        let key = Pubkey::new_unique();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(
            Account::load_as::<Asset>(&account),
            Err(ProgramError::from(RoshiError::InvalidAssetAccount))
        );
    }

    #[test]
    fn load_as_rejects_invalid_oracle_kind() {
        let asset = test_asset(true);
        let mut data = serialize(&Account::Asset(asset)).unwrap();
        let oracle_kind_offset = 1
            + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<roshi_interface::oracle::SwitchboardOracleConfig>()
            + core::mem::size_of::<roshi_interface::oracle::PythOracleConfig>();
        data[oracle_kind_offset] = 255;
        let key = Pubkey::new_unique();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);

        assert_eq!(
            Account::load_as::<Asset>(&account),
            Err(ProgramError::from(RoshiError::InvalidAssetAccount))
        );
    }
}
