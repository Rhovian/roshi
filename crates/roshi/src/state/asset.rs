use roshi_interface::oracle::SwitchboardOracleConfig;
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct Asset {
    /// Vault this non-base asset belongs to.
    pub vault: [u8; 32],
    /// Mint of the supported non-base deposit asset.
    pub asset_mint: [u8; 32],
    /// Token account controlled by the vault for this asset mint.
    pub custody_token_account: [u8; 32],
    /// Switchboard oracle config that reports this asset in vault base units.
    pub oracle: SwitchboardOracleConfig,
    /// Asset mint decimals.
    pub asset_decimals: u8,
    /// Vault base mint decimals.
    pub base_decimals: u8,
    /// Optional per-asset circuit-breaker for price moves.
    pub max_price_change_bps: u16,
    /// Maximum deposit amount in asset atomic units. Zero means unlimited.
    pub deposit_limit: u64,
    /// Whether deposits for this asset are enabled.
    pub enabled: bool,
    pub bump: u8,
}

impl Asset {
    pub const SEED: &'static [u8] = b"asset";

    pub fn find_address(vault: &Pubkey, asset_mint: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED, vault.as_ref(), asset_mint.as_ref()],
            &crate::ID,
        )
    }
}
