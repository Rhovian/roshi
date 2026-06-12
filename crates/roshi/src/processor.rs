use crate::{
    instructions::{
        admin::{
            try_authorize_action, try_collect_fees, try_initialize_asset, try_initialize_program,
            try_initialize_vault, try_invest_external, try_process_withdrawals,
            try_register_external_destination, try_report_nav, try_return_external,
            try_revoke_action, try_revoke_external_destination, try_set_nav_authority,
            try_set_pause_flags, try_set_strategist, try_set_swap_authority, try_set_vault_access,
            try_set_withdrawal_authority, try_transfer_program_authority,
            try_transfer_vault_authority, try_update_asset, try_update_vault_config,
            try_write_down_fees,
        },
        execution::{try_atomic_redeem, try_manage, try_manage_batch, try_swap},
        user::{try_cancel_redeem, try_deposit, try_redeem},
        ProcessWithdrawalsArgs, RegisterExternalDestinationArgs, RevokeExternalDestinationArgs,
        RoshiInstruction,
    },
    ID,
};
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

solana_program_entrypoint::entrypoint!(try_process_instruction);

// Accounts are pinned to a single `'info` lifetime (reference lifetime tied to
// the account-data lifetime). `deposit` reads a Switchboard quote, and
// Switchboard's `QuoteVerifier` requires `&'info AccountInfo<'info>`; the
// entrypoint hands accounts that satisfy this, but it must be threaded through
// the dispatcher for the borrow checker to see it.
fn try_process_instruction<'info>(
    program_id: &Pubkey,
    accounts: &'info [AccountInfo<'info>],
    data: &[u8],
) -> ProgramResult {
    if program_id != &ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let instruction =
        RoshiInstruction::decode(data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match instruction {
        RoshiInstruction::InitializeProgram(args) => try_initialize_program(accounts, args),
        RoshiInstruction::InitializeVault(args) => try_initialize_vault(accounts, args),
        RoshiInstruction::AuthorizeAction(args) => try_authorize_action(accounts, args),
        RoshiInstruction::RevokeAction(args) => try_revoke_action(accounts, args),
        RoshiInstruction::Manage(args) => try_manage(accounts, args),
        RoshiInstruction::ManageBatch(args) => try_manage_batch(accounts, args),
        RoshiInstruction::AtomicRedeem(args) => try_atomic_redeem(accounts, args),
        RoshiInstruction::Swap(args) => try_swap(accounts, args),
        RoshiInstruction::ReportNav(args) => try_report_nav(accounts, args),
        RoshiInstruction::Deposit(args) => try_deposit(accounts, args),
        RoshiInstruction::Redeem(args) => try_redeem(accounts, args),
        RoshiInstruction::CancelRedeem(args) => try_cancel_redeem(accounts, args),
        RoshiInstruction::ProcessWithdrawals => {
            try_process_withdrawals(accounts, ProcessWithdrawalsArgs)
        }
        RoshiInstruction::UpdateVaultConfig(args) => try_update_vault_config(accounts, args),
        RoshiInstruction::InitializeAsset(args) => try_initialize_asset(accounts, args),
        RoshiInstruction::UpdateAsset(args) => try_update_asset(accounts, args),
        RoshiInstruction::InvestExternal(args) => try_invest_external(accounts, args),
        RoshiInstruction::ReturnExternal(args) => try_return_external(accounts, args),
        RoshiInstruction::SetPauseFlags(args) => try_set_pause_flags(accounts, args),
        RoshiInstruction::SetVaultAccess(args) => try_set_vault_access(accounts, args),
        RoshiInstruction::TransferProgramAuthority(args) => {
            try_transfer_program_authority(accounts, args)
        }
        RoshiInstruction::TransferVaultAuthority(args) => {
            try_transfer_vault_authority(accounts, args)
        }
        RoshiInstruction::SetStrategist(args) => try_set_strategist(accounts, args),
        RoshiInstruction::SetSwapAuthority(args) => try_set_swap_authority(accounts, args),
        RoshiInstruction::SetNavAuthority(args) => try_set_nav_authority(accounts, args),
        RoshiInstruction::SetWithdrawalAuthority(args) => {
            try_set_withdrawal_authority(accounts, args)
        }
        RoshiInstruction::CollectFees(args) => try_collect_fees(accounts, args),
        RoshiInstruction::WriteDownFees(args) => try_write_down_fees(accounts, args),
        RoshiInstruction::RegisterExternalDestination => {
            try_register_external_destination(accounts, RegisterExternalDestinationArgs)
        }
        RoshiInstruction::RevokeExternalDestination => {
            try_revoke_external_destination(accounts, RevokeExternalDestinationArgs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::instructions::{serialize_instruction, DepositArgs, InstructionArgs};

    #[test]
    fn decodes_payload_from_encoded_instruction() {
        let encoded = serialize_instruction(&DepositArgs {
            asset_mint: [1; 32],
            amount: 123,
            min_shares_out: 456,
            access_proof: vec![[2; 32]],
        })
        .unwrap();

        let RoshiInstruction::Deposit(args) = RoshiInstruction::decode(&encoded).unwrap() else {
            panic!("expected deposit instruction");
        };

        assert_eq!(args.asset_mint, [1; 32]);
        assert_eq!(args.amount, 123);
        assert_eq!(args.min_shares_out, 456);
        assert_eq!(args.access_proof, vec![[2; 32]]);
    }

    #[test]
    fn decoded_payload_rejects_extra_bytes() {
        let mut encoded = serialize_instruction(&DepositArgs {
            asset_mint: [1; 32],
            amount: 123,
            min_shares_out: 456,
            access_proof: vec![],
        })
        .unwrap();
        encoded.push(0);

        assert!(RoshiInstruction::decode(&encoded).is_err());
    }

    #[test]
    fn rejects_missing_or_unknown_instruction_tag() {
        assert_eq!(
            try_process_instruction(&ID, &[], &[]),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            try_process_instruction(&ID, &[], &[255]),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test]
    fn process_withdrawals_rejects_payload_bytes() {
        assert_eq!(
            try_process_instruction(
                &ID,
                &[],
                &[<ProcessWithdrawalsArgs as InstructionArgs>::TAG, 0],
            ),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
