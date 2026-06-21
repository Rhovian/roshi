use roshi_interface::instructions::{
    CollectFeesArgs, InitializeVaultArgs, InvestExternalArgs, ProcessWithdrawalsArgs,
    RegisterExternalDestinationArgs, ReportNavArgs, ReturnExternalArgs,
    RevokeExternalDestinationArgs, SetNavAuthorityArgs, SetPauseFlagsArgs, SetShareMetadataArgs,
    SetStrategistArgs, SetVaultAccessArgs, SetWithdrawalAuthorityArgs,
    TransferProgramAuthorityArgs, TransferVaultAuthorityArgs, UpdateVaultConfigArgs,
    WriteDownFeesArgs,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result, TOKEN_PROGRAM_ID};

pub fn initialize_vault(
    program_authority: Pubkey,
    program_config: Pubkey,
    payer: Pubkey,
    vault: Pubkey,
    args: InitializeVaultArgs,
) -> Result<Instruction> {
    let base_mint = Pubkey::from(args.base_mint);
    let share_mint = roshi_interface::find_share_mint_address(&vault).0;
    let treasury = Pubkey::from(args.treasury);
    new(
        vec![
            AccountMeta::new_readonly(program_authority, true),
            AccountMeta::new_readonly(program_config, false),
            AccountMeta::new(payer, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(base_mint, false),
            AccountMeta::new(share_mint, false),
            AccountMeta::new_readonly(treasury, false),
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
    let treasury = Pubkey::from(args.treasury);
    new(
        vec![
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(treasury, false),
        ],
        &args,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn report_nav(
    nav_authority: Pubkey,
    vault: Pubkey,
    share_mint: Pubkey,
    base_mint: Pubkey,
    deposit_base_custody: Pubkey,
    withdraw_base_custody: Pubkey,
    external_value: u64,
    report_hash: [u8; 32],
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(nav_authority, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(share_mint, false),
            AccountMeta::new_readonly(base_mint, false),
            AccountMeta::new_readonly(deposit_base_custody, false),
            AccountMeta::new_readonly(withdraw_base_custody, false),
        ],
        &ReportNavArgs {
            external_value,
            report_hash,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn set_share_metadata(
    admin: Pubkey,
    vault: Pubkey,
    share_mint: Pubkey,
    metadata: Pubkey,
    token_metadata_program: Pubkey,
    name: String,
    symbol: String,
    uri: String,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(share_mint, false),
            AccountMeta::new(metadata, false),
            AccountMeta::new_readonly(token_metadata_program, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &SetShareMetadataArgs { name, symbol, uri },
    )
}

pub fn write_down_fees(admin: Pubkey, vault: Pubkey, amount: u64) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        &WriteDownFeesArgs { amount },
    )
}

pub fn register_external_destination(
    admin: Pubkey,
    vault: Pubkey,
    destination_token_account: Pubkey,
    external_destination: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(destination_token_account, false),
            AccountMeta::new(external_destination, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &RegisterExternalDestinationArgs,
    )
}

pub fn revoke_external_destination(
    admin: Pubkey,
    vault: Pubkey,
    external_destination: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(external_destination, false),
        ],
        &RevokeExternalDestinationArgs,
    )
}

pub fn collect_fees(
    admin: Pubkey,
    vault: Pubkey,
    fee_sub_account_index: u8,
    fee_sub_account: Pubkey,
    custody: Pubkey,
    treasury: Pubkey,
    amount: u64,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(fee_sub_account, false),
            AccountMeta::new(custody, false),
            AccountMeta::new(treasury, false),
            AccountMeta::new_readonly(super::TOKEN_PROGRAM_ID, false),
        ],
        &CollectFeesArgs {
            sub_account: fee_sub_account_index,
            amount,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn invest_external(
    strategist: Pubkey,
    vault: Pubkey,
    sub_account_index: u8,
    sub_account: Pubkey,
    custody: Pubkey,
    external_account: Pubkey,
    external_destination: Pubkey,
    amount: u64,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(strategist, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sub_account, false),
            AccountMeta::new(custody, false),
            AccountMeta::new(external_account, false),
            AccountMeta::new_readonly(external_destination, false),
            AccountMeta::new_readonly(super::TOKEN_PROGRAM_ID, false),
        ],
        &InvestExternalArgs {
            sub_account: sub_account_index,
            amount,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn return_external(
    strategist: Pubkey,
    external_authority: Pubkey,
    vault: Pubkey,
    sub_account_index: u8,
    sub_account: Pubkey,
    external_account: Pubkey,
    custody: Pubkey,
    amount: u64,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(strategist, true),
            AccountMeta::new_readonly(external_authority, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sub_account, false),
            AccountMeta::new(external_account, false),
            AccountMeta::new(custody, false),
            AccountMeta::new_readonly(super::TOKEN_PROGRAM_ID, false),
        ],
        &ReturnExternalArgs {
            sub_account: sub_account_index,
            amount,
        },
    )
}

pub fn process_withdrawals(
    withdrawal_authority: Pubkey,
    vault: Pubkey,
    withdraw_sub_account: Pubkey,
    custody: Pubkey,
    share_mint: Pubkey,
    settlements: Vec<(Pubkey, Pubkey, Pubkey)>,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new_readonly(withdrawal_authority, true),
        AccountMeta::new(vault, false),
        AccountMeta::new_readonly(withdraw_sub_account, false),
        AccountMeta::new(custody, false),
        AccountMeta::new_readonly(share_mint, false),
        AccountMeta::new_readonly(super::TOKEN_PROGRAM_ID, false),
    ];

    for (ticket, owner, destination) in settlements {
        accounts.push(AccountMeta::new(ticket, false));
        accounts.push(AccountMeta::new(owner, false));
        accounts.push(AccountMeta::new(destination, false));
    }

    new(accounts, &ProcessWithdrawalsArgs)
}
