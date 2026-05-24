use crate::{
    instructions::{
        authorize_action::try_authorize_action, claim::try_claim, deposit::try_deposit,
        initialize_program::try_initialize_program, initialize_vault::try_initialize_vault,
        manage::try_manage, manage_batch::try_manage_batch, pause_vault::try_pause_vault,
        process_epoch::try_process_epoch, redeem::try_redeem, resume_vault::try_resume_vault,
        revoke_action::try_revoke_action, update_fee_config::try_update_fee_config,
        update_operator::try_update_operator, update_queue_authority::try_update_queue_authority,
        update_total_assets::try_update_total_assets, RoshiInstruction,
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
            program_id,
            accounts_start,
            accounts_len,
            ix_data,
        } => try_manage(accounts, program_id, accounts_start, accounts_len, ix_data),
        RoshiInstruction::ManageBatch { actions } => try_manage_batch(accounts, actions),
        RoshiInstruction::UpdateTotalAssets {
            total_assets,
            external_assets,
        } => try_update_total_assets(accounts, total_assets, external_assets),
        RoshiInstruction::Deposit {
            amount,
            min_shares_out,
        } => try_deposit(accounts, amount, min_shares_out),
        RoshiInstruction::Redeem {
            shares,
            min_assets_out,
        } => try_redeem(accounts, shares, min_assets_out),
        RoshiInstruction::Claim { epoch } => try_claim(accounts, epoch),
        RoshiInstruction::ProcessEpoch { epoch } => try_process_epoch(accounts, epoch),
        RoshiInstruction::UpdateOperator { operator } => try_update_operator(accounts, operator),
        RoshiInstruction::UpdateQueueAuthority { queue_authority } => {
            try_update_queue_authority(accounts, queue_authority)
        }
        RoshiInstruction::UpdateFeeConfig {
            performance_fee_bps,
            fee_collector,
        } => try_update_fee_config(accounts, performance_fee_bps, fee_collector),
        RoshiInstruction::PauseVault {
            deposits_paused,
            withdrawals_paused,
        } => try_pause_vault(accounts, deposits_paused, withdrawals_paused),
        RoshiInstruction::ResumeVault {
            deposits,
            withdrawals,
        } => try_resume_vault(accounts, deposits, withdrawals),
    }
}
