pub mod admin;
pub mod args;
pub mod execution;
pub mod update_total_assets;
pub mod user;

pub use args::{
    IndexedActionArgs, InitializeAssetArgs, InitializeVaultArgs, SetPauseFlagsArgs,
    UpdateAssetArgs, UpdateVaultConfigArgs,
};

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
        sub_account: u8,
        program_id: [u8; 32],
        accounts_start: u8,
        accounts_len: u8,
        ix_data: Vec<u8>,
    },
    #[wincode(tag = 5)]
    ManageBatch { actions: Vec<IndexedActionArgs> },
    #[wincode(tag = 6)]
    UpdateTotalAssets {
        total_assets: u64,
        report_hash: [u8; 32],
    },
    #[wincode(tag = 7)]
    Deposit {
        asset_mint: [u8; 32],
        amount: u64,
        min_shares_out: u64,
    },
    #[wincode(tag = 8)]
    Redeem {
        ticket_index: u8,
        shares: u64,
        min_assets_out: u64,
    },
    #[wincode(tag = 10)]
    ProcessWithdrawals,
    #[wincode(tag = 11)]
    UpdateVaultConfig { args: UpdateVaultConfigArgs },
    #[wincode(tag = 12)]
    InitializeAsset { args: InitializeAssetArgs },
    #[wincode(tag = 13)]
    UpdateAsset { args: UpdateAssetArgs },
    #[wincode(tag = 14)]
    InitializeSubAccount { index: u8 },
    #[wincode(tag = 15)]
    SetPauseFlags { args: SetPauseFlagsArgs },
}
