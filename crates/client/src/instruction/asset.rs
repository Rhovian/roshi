use roshi_interface::instructions::{InitializeAssetArgs, UpdateAssetArgs};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result};

pub fn initialize_asset(
    admin: Pubkey,
    vault: Pubkey,
    asset_mint: Pubkey,
    asset: Pubkey,
    args: InitializeAssetArgs,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(asset_mint, false),
            AccountMeta::new(asset, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &args,
    )
}

pub fn update_asset(
    admin: Pubkey,
    vault: Pubkey,
    asset: Pubkey,
    args: UpdateAssetArgs,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(asset, false),
        ],
        &args,
    )
}
