use roshi_interface::{
    action::{ActionScope, Ops},
    instructions::{
        AdminSetFlashFeeRateArgs, AuthorizeActionArgs, RevokeActionArgs,
        StrategistLowerFlashFeeRateArgs,
    },
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result};

#[allow(clippy::too_many_arguments)]
pub fn authorize_action(
    admin: Pubkey,
    vault: Pubkey,
    action: Pubkey,
    action_hash: [u8; 32],
    scope: ActionScope,
    ops: Ops,
    redeem_amount_offset: u16,
    fee_num: u64,
    fee_den: u64,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(action, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &AuthorizeActionArgs {
            action_hash,
            scope,
            ops,
            redeem_amount_offset,
            fee_num,
            fee_den,
        },
    )
}

/// Admin-gated: set a `FlashApprove` action's committed flash-fee rate to any
/// value (raising it is a theft lever, so it is admin-only — #22).
pub fn admin_set_flash_fee_rate(
    admin: Pubkey,
    vault: Pubkey,
    action: Pubkey,
    fee_num: u64,
    fee_den: u64,
) -> Result<Instruction> {
    new(
        flash_fee_rate_accounts(admin, vault, action),
        &AdminSetFlashFeeRateArgs { fee_num, fee_den },
    )
}

/// Strategist-gated: lower a `FlashApprove` action's committed flash-fee rate to
/// a value strictly below the current one (lowering is fail-safe — #22).
pub fn strategist_lower_flash_fee_rate(
    strategist: Pubkey,
    vault: Pubkey,
    action: Pubkey,
    fee_num: u64,
    fee_den: u64,
) -> Result<Instruction> {
    new(
        flash_fee_rate_accounts(strategist, vault, action),
        &StrategistLowerFlashFeeRateArgs { fee_num, fee_den },
    )
}

fn flash_fee_rate_accounts(authority: Pubkey, vault: Pubkey, action: Pubkey) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new_readonly(authority, true),
        AccountMeta::new_readonly(vault, false),
        AccountMeta::new(action, false),
    ]
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
