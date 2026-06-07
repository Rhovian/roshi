use roshi_interface::instructions::{AtomicRedeemArgs, ManageArgs, ManageBatchArgs};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use super::TOKEN_PROGRAM_ID;
use super::{new, Result};

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
        AccountMeta::new_readonly(sub_account_pda, false),
        AccountMeta::new_readonly(action, false),
        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
    ];
    accounts.extend(cpi_accounts);

    new(accounts, &args)
}
