pub mod authorize_action;
pub mod claim;
pub mod deposit;
pub mod initialize_program;
pub mod initialize_vault;
pub mod manage;
pub mod manage_batch;
pub mod process_withdrawals;
pub mod redeem;
pub mod revoke_action;
pub mod update_total_assets;
pub mod update_vault_config;

use crate::state::action::Ops;
use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead)]
#[wincode(tag_encoding = "u8")]
pub enum RoshiInstruction {
    #[wincode(tag = 0)]
    InitializeProgram { authority: [u8; 32] },
    #[wincode(tag = 1)]
    InitializeVault { args: InitializeVaultArgs },
    #[wincode(tag = 2)]
    AuthorizeAction { action_hash: [u8; 32], ops: Ops },
    #[wincode(tag = 3)]
    RevokeAction { action_hash: [u8; 32] },
    #[wincode(tag = 4)]
    Manage {
        program_id: [u8; 32],
        accounts_start: u8,
        accounts_len: u8,
        ix_data: Vec<u8>,
    },
    #[wincode(tag = 5)]
    ManageBatch { actions: Vec<IndexedActionArgs> },
    #[wincode(tag = 6)]
    UpdateTotalAssets { external_assets: u64 },
    #[wincode(tag = 7)]
    Deposit { amount: u64, min_shares_out: u64 },
    #[wincode(tag = 8)]
    Redeem {
        ticket_index: u8,
        shares: u64,
        min_assets_out: u64,
    },
    #[wincode(tag = 9)]
    Claim,
    #[wincode(tag = 10)]
    ProcessWithdrawals,
    #[wincode(tag = 11)]
    UpdateVaultConfig { args: UpdateVaultConfigArgs },
}

#[derive(SchemaWrite, SchemaRead)]
pub struct IndexedActionArgs {
    pub program_id: [u8; 32],
    pub accounts_start: u8,
    pub accounts_len: u8,
    pub ix_data: Vec<u8>,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct InitializeVaultArgs {
    pub admin: [u8; 32],
    pub operator: [u8; 32],
    pub queue_authority: [u8; 32],
    pub base_mint: [u8; 32],
    pub share_mint: [u8; 32],
    pub vault_token_account: [u8; 32],
    pub fee_collector: [u8; 32],
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub max_change_bps: u16,
    pub min_update_interval: i64,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct UpdateVaultConfigArgs {
    pub operator: [u8; 32],
    pub queue_authority: [u8; 32],
    pub fee_collector: [u8; 32],
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub max_change_bps: u16,
    pub min_update_interval: i64,
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
}
