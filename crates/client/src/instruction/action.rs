use roshi_interface::{
    action::Ops,
    instructions::{AuthorizeActionArgs, RevokeActionArgs},
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result};

pub fn authorize_action(
    admin: Pubkey,
    vault: Pubkey,
    action: Pubkey,
    action_hash: [u8; 32],
    ops: Ops,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(action, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &AuthorizeActionArgs { action_hash, ops },
    )
}

pub fn revoke_action(
    admin: Pubkey,
    vault: Pubkey,
    action: Pubkey,
    action_hash: [u8; 32],
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(action, false),
        ],
        &RevokeActionArgs { action_hash },
    )
}
