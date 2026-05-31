use crate::{
    instructions::{
        admin::{
            try_authorize_action, try_initialize_asset, try_initialize_program,
            try_initialize_sub_account, try_initialize_vault, try_process_withdrawals,
            try_revoke_action, try_set_pause_flags, try_set_vault_access, try_update_asset,
            try_update_vault_config,
        },
        execution::{try_manage, try_manage_batch},
        update_total_assets::try_update_total_assets,
        user::{try_deposit, try_redeem},
        RoshiInstruction,
    },
    ID,
};
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::deserialize;

solana_program_entrypoint::entrypoint!(try_process_instruction);

fn try_process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if program_id != &ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let ix_data = deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match ix_data {
        RoshiInstruction::InitializeProgram { authority } => {
            try_initialize_program(accounts, authority)
        }
        RoshiInstruction::InitializeVault { args } => try_initialize_vault(accounts, args),
        RoshiInstruction::AuthorizeAction { action_hash, ops } => {
            try_authorize_action(accounts, action_hash, ops)
        }
        RoshiInstruction::RevokeAction { action_hash } => try_revoke_action(accounts, action_hash),
        RoshiInstruction::Manage {
            sub_account,
            program_id,
            accounts_start,
            accounts_len,
            ix_data,
        } => try_manage(
            accounts,
            sub_account,
            program_id,
            accounts_start,
            accounts_len,
            ix_data,
        ),
        RoshiInstruction::ManageBatch { actions } => try_manage_batch(accounts, actions),
        RoshiInstruction::UpdateTotalAssets {
            total_assets,
            report_hash,
        } => try_update_total_assets(accounts, total_assets, report_hash),
        RoshiInstruction::Deposit {
            asset_mint,
            amount,
            min_shares_out,
            access_proof,
        } => try_deposit(accounts, asset_mint, amount, min_shares_out, access_proof),
        RoshiInstruction::Redeem {
            ticket_index,
            shares,
            min_assets_out,
        } => try_redeem(accounts, ticket_index, shares, min_assets_out),
        RoshiInstruction::ProcessWithdrawals => try_process_withdrawals(accounts),
        RoshiInstruction::UpdateVaultConfig { args } => try_update_vault_config(accounts, args),
        RoshiInstruction::InitializeAsset { args } => try_initialize_asset(accounts, args),
        RoshiInstruction::UpdateAsset { args } => try_update_asset(accounts, args),
        RoshiInstruction::InitializeSubAccount { index } => {
            try_initialize_sub_account(accounts, index)
        }
        RoshiInstruction::SetPauseFlags { args } => try_set_pause_flags(accounts, args),
        RoshiInstruction::SetVaultAccess { args } => try_set_vault_access(accounts, args),
    }
}
