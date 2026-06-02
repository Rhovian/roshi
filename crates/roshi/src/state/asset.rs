use roshi_interface::{error::RoshiError, oracle::OracleConfig};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

use crate::state::flags;

#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct Asset {
    /// Vault this non-base asset belongs to.
    pub vault: [u8; 32],
    /// Mint of the supported non-base deposit asset.
    pub asset_mint: [u8; 32],
    /// Oracle config that reports this asset in vault base atoms.
    pub oracle: OracleConfig,
    /// Asset mint decimals.
    pub asset_decimals: u8,
    /// Whether deposits for this asset are enabled.
    enabled_flag: u8,
    pub bump: u8,
    _padding: [u8; 5],
}

impl Asset {
    pub const SEED: &'static [u8] = b"asset";
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    pub fn new(
        vault: [u8; 32],
        asset_mint: [u8; 32],
        oracle: OracleConfig,
        asset_decimals: u8,
        enabled: bool,
        bump: u8,
    ) -> Result<Self, ProgramError> {
        oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidAssetAccount))?;

        Ok(Self {
            vault,
            asset_mint,
            oracle,
            asset_decimals,
            enabled_flag: flags::bool_to_flag(enabled),
            bump,
            _padding: [0; 5],
        })
    }

    pub fn find_address(vault: &Pubkey, asset_mint: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED, vault.as_ref(), asset_mint.as_ref()],
            &crate::ID,
        )
    }

    pub fn enabled(&self) -> Result<bool, ProgramError> {
        flags::flag_to_bool(self.enabled_flag, RoshiError::InvalidAssetAccount)
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled_flag = flags::bool_to_flag(enabled);
    }

    pub fn validate_state(&self) -> ProgramResult {
        self.oracle
            .validate()
            .map_err(|_| ProgramError::from(RoshiError::InvalidAssetAccount))?;
        flags::validate_flag(self.enabled_flag, RoshiError::InvalidAssetAccount)
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
        Asset::new([1; 32], [2; 32], OracleConfig::default(), 6, enabled, 7).unwrap()
    }

    #[test]
    fn asset_is_zero_copy_with_explicit_padding() {
        let asset = test_asset(true);

        assert_zero_copy::<Asset>();
        assert_eq!(core::mem::size_of::<Asset>(), 240);
        assert_eq!(Asset::SPACE, 241);
        assert_eq!(
            serialize(&asset).unwrap().len(),
            core::mem::size_of::<Asset>()
        );
    }

    #[test]
    fn enabled_flag_uses_typed_accessors() {
        let mut asset = test_asset(false);

        assert_eq!(asset.enabled(), Ok(false));
        asset.set_enabled(true);
        assert_eq!(asset.enabled(), Ok(true));
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
