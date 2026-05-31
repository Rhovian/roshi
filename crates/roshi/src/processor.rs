use crate::{
    instructions::{
        admin::{
            try_authorize_action, try_initialize_asset, try_initialize_program,
            try_initialize_sub_account, try_initialize_vault, try_process_withdrawals,
            try_revoke_action, try_set_nav_authority, try_set_pause_flags, try_set_strategist,
            try_set_vault_access, try_set_withdrawal_authority, try_transfer_program_authority,
            try_transfer_vault_authority, try_update_asset, try_update_vault_config,
        },
        execution::{try_manage, try_manage_batch},
        user::{try_deposit, try_redeem},
        AuthorizeActionArgs, DepositArgs, InitializeAssetArgs, InitializeProgramArgs,
        InitializeSubAccountArgs, InitializeVaultArgs, ManageArgs, ManageBatchArgs,
        ProcessWithdrawalsArgs, RedeemArgs, RevokeActionArgs, RoshiInstructionTag,
        SetNavAuthorityArgs, SetPauseFlagsArgs, SetStrategistArgs, SetVaultAccessArgs,
        SetWithdrawalAuthorityArgs, TransferProgramAuthorityArgs, TransferVaultAuthorityArgs,
        UpdateAssetArgs, UpdateVaultConfigArgs,
    },
    ID,
};
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{config::DefaultConfig, deserialize_exact, SchemaRead};

solana_program_entrypoint::entrypoint!(try_process_instruction);

fn split_instruction_data(data: &[u8]) -> Result<(RoshiInstructionTag, &[u8]), ProgramError> {
    let (tag, payload) = data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;
    let tag =
        RoshiInstructionTag::try_from(*tag).map_err(|_| ProgramError::InvalidInstructionData)?;

    Ok((tag, payload))
}

fn decode_payload<'a, T>(payload: &'a [u8]) -> Result<T, ProgramError>
where
    T: SchemaRead<'a, DefaultConfig, Dst = T>,
{
    deserialize_exact(payload).map_err(|_| ProgramError::InvalidInstructionData)
}

macro_rules! decode_and_process {
    ($handler:ident, $accounts:expr, $payload:expr, $args:ty) => {{
        let args: $args = decode_payload($payload)?;
        $handler($accounts, args)
    }};
}

fn try_process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if program_id != &ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (tag, payload) = split_instruction_data(data)?;

    match tag {
        RoshiInstructionTag::InitializeProgram => {
            decode_and_process!(
                try_initialize_program,
                accounts,
                payload,
                InitializeProgramArgs
            )
        }
        RoshiInstructionTag::InitializeVault => {
            decode_and_process!(try_initialize_vault, accounts, payload, InitializeVaultArgs)
        }
        RoshiInstructionTag::AuthorizeAction => {
            decode_and_process!(try_authorize_action, accounts, payload, AuthorizeActionArgs)
        }
        RoshiInstructionTag::RevokeAction => {
            decode_and_process!(try_revoke_action, accounts, payload, RevokeActionArgs)
        }
        RoshiInstructionTag::Manage => {
            decode_and_process!(try_manage, accounts, payload, ManageArgs)
        }
        RoshiInstructionTag::ManageBatch => {
            decode_and_process!(try_manage_batch, accounts, payload, ManageBatchArgs)
        }
        RoshiInstructionTag::Deposit => {
            decode_and_process!(try_deposit, accounts, payload, DepositArgs)
        }
        RoshiInstructionTag::Redeem => {
            decode_and_process!(try_redeem, accounts, payload, RedeemArgs)
        }
        RoshiInstructionTag::ProcessWithdrawals => {
            decode_and_process!(
                try_process_withdrawals,
                accounts,
                payload,
                ProcessWithdrawalsArgs
            )
        }
        RoshiInstructionTag::UpdateVaultConfig => {
            decode_and_process!(
                try_update_vault_config,
                accounts,
                payload,
                UpdateVaultConfigArgs
            )
        }
        RoshiInstructionTag::InitializeAsset => {
            decode_and_process!(try_initialize_asset, accounts, payload, InitializeAssetArgs)
        }
        RoshiInstructionTag::UpdateAsset => {
            decode_and_process!(try_update_asset, accounts, payload, UpdateAssetArgs)
        }
        RoshiInstructionTag::InitializeSubAccount => {
            decode_and_process!(
                try_initialize_sub_account,
                accounts,
                payload,
                InitializeSubAccountArgs
            )
        }
        RoshiInstructionTag::SetPauseFlags => {
            decode_and_process!(try_set_pause_flags, accounts, payload, SetPauseFlagsArgs)
        }
        RoshiInstructionTag::SetVaultAccess => {
            decode_and_process!(try_set_vault_access, accounts, payload, SetVaultAccessArgs)
        }
        RoshiInstructionTag::TransferProgramAuthority => {
            decode_and_process!(
                try_transfer_program_authority,
                accounts,
                payload,
                TransferProgramAuthorityArgs
            )
        }
        RoshiInstructionTag::TransferVaultAuthority => {
            decode_and_process!(
                try_transfer_vault_authority,
                accounts,
                payload,
                TransferVaultAuthorityArgs
            )
        }
        RoshiInstructionTag::SetStrategist => {
            decode_and_process!(try_set_strategist, accounts, payload, SetStrategistArgs)
        }
        RoshiInstructionTag::SetNavAuthority => {
            decode_and_process!(
                try_set_nav_authority,
                accounts,
                payload,
                SetNavAuthorityArgs
            )
        }
        RoshiInstructionTag::SetWithdrawalAuthority => {
            decode_and_process!(
                try_set_withdrawal_authority,
                accounts,
                payload,
                SetWithdrawalAuthorityArgs
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::instructions::serialize_instruction;

    #[test]
    fn decodes_payload_after_splitting_encoded_instruction() {
        let encoded = serialize_instruction(&DepositArgs {
            asset_mint: [1; 32],
            amount: 123,
            min_shares_out: 456,
            access_proof: vec![[2; 32]],
        })
        .unwrap();

        let (tag, payload) = split_instruction_data(&encoded).unwrap();
        let args: DepositArgs = decode_payload(payload).unwrap();

        assert_eq!(tag, RoshiInstructionTag::Deposit);
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

        let (_, payload) = split_instruction_data(&encoded).unwrap();

        assert!(matches!(
            decode_payload::<DepositArgs>(payload),
            Err(ProgramError::InvalidInstructionData)
        ));
    }

    #[test]
    fn rejects_missing_or_unknown_instruction_tag() {
        assert_eq!(
            try_process_instruction(&ID, &[], &[]),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            try_process_instruction(&ID, &[], &[6]),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test]
    fn process_withdrawals_rejects_payload_bytes() {
        assert_eq!(
            try_process_instruction(
                &ID,
                &[],
                &[u8::from(RoshiInstructionTag::ProcessWithdrawals), 0],
            ),
            Err(ProgramError::InvalidInstructionData)
        );
    }
}
