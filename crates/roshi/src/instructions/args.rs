use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead)]
pub struct IndexedActionArgs {
    pub sub_account: u8,
    pub program_id: [u8; 32],
    pub accounts_start: u8,
    pub accounts_len: u8,
    pub ix_data: Vec<u8>,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct InitializeVaultArgs {
    pub admin: [u8; 32],
    pub strategist: [u8; 32],
    pub nav_authority: [u8; 32],
    pub withdrawal_authority: [u8; 32],
    pub base_mint: [u8; 32],
    pub share_mint: [u8; 32],
    pub deposit_sub_account: u8,
    pub withdraw_sub_account: u8,
    pub fee_collector: [u8; 32],
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub max_change_bps: u16,
    pub min_update_interval: i64,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct InitializeAssetArgs {
    pub asset_mint: [u8; 32],
    pub custody_token_account: [u8; 32],
    pub oracle: [u8; 32],
    pub oracle_type: u8,
    pub asset_decimals: u8,
    pub base_decimals: u8,
    pub oracle_max_age: i64,
    pub max_price_change_bps: u16,
    pub deposit_limit: u64,
    pub enabled: bool,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct UpdateAssetArgs {
    pub custody_token_account: [u8; 32],
    pub oracle: [u8; 32],
    pub oracle_type: u8,
    pub oracle_max_age: i64,
    pub max_price_change_bps: u16,
    pub deposit_limit: u64,
    pub enabled: bool,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct UpdateVaultConfigArgs {
    pub strategist: [u8; 32],
    pub nav_authority: [u8; 32],
    pub withdrawal_authority: [u8; 32],
    pub fee_collector: [u8; 32],
    pub deposit_sub_account: u8,
    pub withdraw_sub_account: u8,
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub max_change_bps: u16,
    pub min_update_interval: i64,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct SetPauseFlagsArgs {
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
    pub manage_paused: bool,
}
