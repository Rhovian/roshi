//! `Asset` account wire type and decode helpers — a vault's per-asset oracle
//! registration for a non-base deposit asset (off-chain readers decode it to learn
//! how the asset prices, including whether it routes through the vault `base_oracle`).

use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{deserialize, SchemaRead, SchemaWrite};

use crate::{error::RoshiError, oracle::OracleConfig, state::ASSET_ACCOUNT_TAG, ID};

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
        Pubkey::find_program_address(&[Self::SEED, vault.as_ref(), asset_mint.as_ref()], &ID)
    }

    /// Decode an `Asset` from raw Roshi account data — the wincode `Account::Asset`
    /// payload (a one-byte tag then the asset). Mirrors [`super::Vault::from_account_data`].
    pub fn from_account_data(data: &[u8]) -> Result<Self, ProgramError> {
        let (&tag, rest) = data
            .split_first()
            .ok_or(ProgramError::from(RoshiError::InvalidAssetAccount))?;
        if tag != ASSET_ACCOUNT_TAG {
            return Err(RoshiError::InvalidAssetAccount.into());
        }
        let asset: Self =
            deserialize(rest).map_err(|_| ProgramError::from(RoshiError::InvalidAssetAccount))?;
        asset.validate_state()?;
        Ok(asset)
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
    use wincode::{config::DefaultConfig, serialize, TypeMeta};

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

    fn test_asset(enabled: bool, routed: bool) -> Asset {
        Asset::new(
            [1; 32],
            [2; 32],
            OracleConfig::default(),
            6,
            enabled,
            routed,
            u64::MAX,
            7,
        )
        .unwrap()
    }

    /// The tagged account payload `from_account_data` consumes (tag byte + asset).
    fn account_data(asset: &Asset) -> Vec<u8> {
        let mut data = vec![ASSET_ACCOUNT_TAG];
        data.extend_from_slice(&serialize(asset).unwrap());
        data
    }

    /// Offset of `routed_flag` in the tagged payload — after the leading tag, the two
    /// pubkeys, the oracle, the cap, the decimals, and `enabled_flag`.
    fn routed_flag_offset() -> usize {
        1 + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<OracleConfig>()
            + core::mem::size_of::<u64>()
            + core::mem::size_of::<u8>()
            + core::mem::size_of::<u8>()
    }

    #[test]
    fn asset_is_zero_copy_with_explicit_padding() {
        let asset = test_asset(true, false);
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
        let mut asset = test_asset(false, false);
        assert_eq!(asset.enabled(), Ok(false));
        asset.set_enabled(true);
        assert_eq!(asset.enabled(), Ok(true));
        assert_eq!(asset.routed(), Ok(false));
        asset.set_routed(true);
        assert_eq!(asset.routed(), Ok(true));
    }

    #[test]
    fn from_account_data_round_trips_a_valid_asset() {
        let asset = test_asset(true, true);
        let decoded = Asset::from_account_data(&account_data(&asset)).unwrap();
        assert_eq!(decoded, asset);
        assert_eq!(decoded.routed(), Ok(true));
    }

    #[test]
    fn from_account_data_rejects_a_bad_tag() {
        let mut data = account_data(&test_asset(true, false));
        data[0] = ASSET_ACCOUNT_TAG.wrapping_add(1);
        assert_eq!(
            Asset::from_account_data(&data),
            Err(ProgramError::from(RoshiError::InvalidAssetAccount))
        );
    }

    #[test]
    fn from_account_data_rejects_an_invalid_routed_flag() {
        let mut data = account_data(&test_asset(true, false));
        data[routed_flag_offset()] = 255;
        assert_eq!(
            Asset::from_account_data(&data),
            Err(ProgramError::from(RoshiError::InvalidAssetAccount))
        );
    }
}
