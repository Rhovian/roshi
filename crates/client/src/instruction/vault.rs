use roshi_interface::instructions::{
    InitializeVaultArgs, SetNavAuthorityArgs, SetPauseFlagsArgs, SetStrategistArgs,
    SetVaultAccessArgs, SetWithdrawalAuthorityArgs, TransferProgramAuthorityArgs,
    TransferVaultAuthorityArgs, UpdateVaultConfigArgs,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result};

pub fn initialize_vault(
    program_authority: Pubkey,
    program_config: Pubkey,
    payer: Pubkey,
    vault: Pubkey,
    args: InitializeVaultArgs,
) -> Result<Instruction> {
    let base_mint = Pubkey::from(args.base_mint);
    let share_mint = Pubkey::from(args.share_mint);
    new(
        vec![
            AccountMeta::new_readonly(program_authority, true),
            AccountMeta::new_readonly(program_config, false),
            AccountMeta::new(payer, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(base_mint, false),
            AccountMeta::new_readonly(share_mint, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &args,
    )
}

pub fn transfer_program_authority(
    authority: Pubkey,
    program_config: Pubkey,
    new_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(program_config, false),
        ],
        &TransferProgramAuthorityArgs {
            new_authority: new_authority.to_bytes(),
        },
    )
}

pub fn transfer_vault_authority(
    authority: Pubkey,
    vault: Pubkey,
    new_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(vault, false),
        ],
        &TransferVaultAuthorityArgs {
            new_authority: new_authority.to_bytes(),
        },
    )
}

fn vault_admin_accounts(admin: Pubkey, vault: Pubkey) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new_readonly(admin, true),
        AccountMeta::new(vault, false),
    ]
}

pub fn set_strategist(admin: Pubkey, vault: Pubkey, strategist: Pubkey) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        &SetStrategistArgs {
            strategist: strategist.to_bytes(),
        },
    )
}

pub fn set_nav_authority(
    admin: Pubkey,
    vault: Pubkey,
    nav_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        &SetNavAuthorityArgs {
            nav_authority: nav_authority.to_bytes(),
        },
    )
}

pub fn set_withdrawal_authority(
    admin: Pubkey,
    vault: Pubkey,
    withdrawal_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        &SetWithdrawalAuthorityArgs {
            withdrawal_authority: withdrawal_authority.to_bytes(),
        },
    )
}

pub fn set_vault_access(
    admin: Pubkey,
    vault: Pubkey,
    private: bool,
    access_merkle_root: [u8; 32],
) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        &SetVaultAccessArgs {
            private,
            access_merkle_root,
        },
    )
}

pub fn set_pause_flags(
    admin: Pubkey,
    vault: Pubkey,
    deposits_paused: bool,
    withdrawals_paused: bool,
    manage_paused: bool,
) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        &SetPauseFlagsArgs {
            deposits_paused,
            withdrawals_paused,
            manage_paused,
        },
    )
}

pub fn update_vault_config(
    admin: Pubkey,
    vault: Pubkey,
    args: UpdateVaultConfigArgs,
) -> Result<Instruction> {
    new(vault_admin_accounts(admin, vault), &args)
}
