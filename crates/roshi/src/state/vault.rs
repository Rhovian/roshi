use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct Vault {
    pub admin: [u8; 32],
    pub operator: [u8; 32],
    pub queue_authority: [u8; 32],
    pub base_mint: [u8; 32],
    pub share_mint: [u8; 32],
    pub vault_token_account: [u8; 32],
    pub fee_collector: [u8; 32],
    pub total_assets: u64,
    pub external_assets: u64,
    pub total_shares: u64,
    pub high_watermark: u64,
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub max_change_bps: u16,
    pub min_update_interval: i64,
    pub last_update_ts: i64,
    pub current_epoch: u64,
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
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
}
