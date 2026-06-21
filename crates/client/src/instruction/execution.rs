use roshi_interface::instructions::{
    AssertDelegateClearedArgs, AtomicRedeemArgs, ManageArgs, ManageBatchArgs, SwapArgs,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use super::TOKEN_PROGRAM_ID;
use super::{new, Result};

/// Permissionless backstop: assert `token_account` carries no delegate and zero
/// delegated amount. `FlashApprove` (#21) binds this as a committed sibling after
/// the top-level `flash_repay` over the delegated sub-account ATA.
pub fn assert_delegate_cleared(token_account: Pubkey) -> Result<Instruction> {
    new(
        vec![AccountMeta::new_readonly(token_account, false)],
        &AssertDelegateClearedArgs,
    )
}

pub fn manage(
    executor: Pubkey,
    vault: Pubkey,
    sub_account_pda: Pubkey,
    action: Pubkey,
    cpi_accounts: Vec<AccountMeta>,
    args: ManageArgs,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new_readonly(executor, true),
        AccountMeta::new_readonly(vault, false),
        AccountMeta::new(sub_account_pda, false),
        AccountMeta::new_readonly(action, false),
    ];
    accounts.extend(cpi_accounts);

    new(accounts, &args)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManageBatchActionAccounts {
    pub sub_account_pda: Pubkey,
    pub action: Pubkey,
}

pub fn manage_batch(
    executor: Pubkey,
    vault: Pubkey,
    action_accounts: Vec<ManageBatchActionAccounts>,
    cpi_accounts: Vec<AccountMeta>,
    actions: Vec<ManageArgs>,
) -> Result<Instruction> {
    let mut accounts = Vec::with_capacity(2 + action_accounts.len() * 2 + cpi_accounts.len());
    accounts.push(AccountMeta::new_readonly(executor, true));
    accounts.push(AccountMeta::new_readonly(vault, false));

    for action_accounts in action_accounts {
        accounts.push(AccountMeta::new(action_accounts.sub_account_pda, false));
        accounts.push(AccountMeta::new_readonly(action_accounts.action, false));
    }

    accounts.extend(cpi_accounts);

    new(accounts, &ManageBatchArgs { actions })
}

#[allow(clippy::too_many_arguments)]
pub fn atomic_redeem(
    owner: Pubkey,
    vault: Pubkey,
    user_share_account: Pubkey,
    share_mint: Pubkey,
    recipient_token_account: Pubkey,
    custody: Pubkey,
    base_token_program: Pubkey,
    sub_account_pda: Pubkey,
    action: Pubkey,
    cpi_accounts: Vec<AccountMeta>,
    args: AtomicRedeemArgs,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new(owner, true),
        AccountMeta::new(vault, false),
        AccountMeta::new(user_share_account, false),
        AccountMeta::new(share_mint, false),
        AccountMeta::new(recipient_token_account, false),
        AccountMeta::new(custody, false),
        AccountMeta::new_readonly(base_token_program, false),
        AccountMeta::new_readonly(sub_account_pda, false),
        AccountMeta::new_readonly(action, false),
        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
    ];
    accounts.extend(cpi_accounts);

    new(accounts, &args)
}

#[allow(clippy::too_many_arguments)]
pub fn swap(
    strategist: Pubkey,
    vault: Pubkey,
    sub_account_pda: Pubkey,
    input_custody: Pubkey,
    output_custody: Pubkey,
    action: Pubkey,
    valuation_accounts: Vec<AccountMeta>,
    cpi_accounts: Vec<AccountMeta>,
    args: SwapArgs,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new_readonly(strategist, true),
        AccountMeta::new_readonly(vault, false),
        AccountMeta::new_readonly(sub_account_pda, false),
        AccountMeta::new(input_custody, false),
        AccountMeta::new(output_custody, false),
        AccountMeta::new_readonly(action, false),
    ];
    accounts.extend(valuation_accounts);
    accounts.extend(cpi_accounts);

    new(accounts, &args)
}
