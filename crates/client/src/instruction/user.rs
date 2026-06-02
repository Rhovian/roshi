use roshi_interface::instructions::{DepositArgs, RedeemArgs};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result, TOKEN_PROGRAM_ID};

#[allow(clippy::too_many_arguments)]
pub fn deposit(
    depositor: Pubkey,
    vault: Pubkey,
    user_source_token_account: Pubkey,
    vault_custody_token_account: Pubkey,
    user_share_account: Pubkey,
    share_mint: Pubkey,
    asset_mint: Pubkey,
    amount: u64,
    min_shares_out: u64,
    access_proof: Vec<[u8; 32]>,
    additional_accounts: Vec<AccountMeta>,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new_readonly(depositor, true),
        AccountMeta::new(vault, false),
        AccountMeta::new(user_source_token_account, false),
        AccountMeta::new(vault_custody_token_account, false),
        AccountMeta::new(user_share_account, false),
        AccountMeta::new(share_mint, false),
        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
    ];
    accounts.extend(additional_accounts);

    new(
        accounts,
        &DepositArgs {
            asset_mint: asset_mint.to_bytes(),
            amount,
            min_shares_out,
            access_proof,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn redeem(
    owner: Pubkey,
    vault: Pubkey,
    user_share_account: Pubkey,
    share_mint: Pubkey,
    recipient_token_account: Pubkey,
    withdrawal_ticket: Pubkey,
    ticket_index: u8,
    shares: u64,
    min_assets_out: u64,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(vault, false),
            AccountMeta::new(user_share_account, false),
            AccountMeta::new(share_mint, false),
            AccountMeta::new_readonly(recipient_token_account, false),
            AccountMeta::new(withdrawal_ticket, false),
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        &RedeemArgs {
            recipient_token_account: recipient_token_account.to_bytes(),
            ticket_index,
            shares,
            min_assets_out,
        },
    )
}
