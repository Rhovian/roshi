use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;
use solana_pubkey::Pubkey;

use crate::instructions::{
    accounts::{next_account, WritableProgramConfigAuthorityContext},
    TransferProgramAuthorityArgs,
};

/// Implements [`crate::instructions::RoshiInstructionTag::TransferProgramAuthority`].
///
/// # Accounts
///
/// 0. `[signer]` Current program authority stored in the program config account.
/// 1. `[writable]` Program config PDA derived from `ProgramConfig::SEED`.
///
/// Verifies the current program authority and replaces it with `new_authority`.
/// The program authority is the protocol-level role allowed to create vaults.
pub fn try_transfer_program_authority(
    accounts: &[AccountInfo],
    args: TransferProgramAuthorityArgs,
) -> ProgramResult {
    let mut accounts_iter = accounts.iter();
    let authority = next_account(&mut accounts_iter)?;
    let program_config_account = next_account(&mut accounts_iter)?;

    let mut context =
        WritableProgramConfigAuthorityContext::load(authority, program_config_account)?;
    context
        .program_config_mut()
        .set_authority(Pubkey::from(args.new_authority));
    context.store()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program_error::ProgramError;
    use wincode::serialize;

    use crate::state::{program_config::ProgramConfig, Account};

    fn program_config_account_data(authority: Pubkey) -> Vec<u8> {
        serialize(&Account::ProgramConfig(ProgramConfig::new(authority))).unwrap()
    }

    fn load_program_config(account: &AccountInfo) -> ProgramConfig {
        Account::load_as::<ProgramConfig>(account).unwrap()
    }

    #[test]
    fn transfers_program_authority() {
        let authority = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let (program_config_key, _) = ProgramConfig::find_address();
        let owner = crate::ID;
        let mut authority_lamports = 1;
        let mut authority_data = [];
        let mut program_config_lamports = 1;
        let mut program_config_data = program_config_account_data(authority);
        let authority_account = AccountInfo::new(
            &authority,
            true,
            false,
            &mut authority_lamports,
            &mut authority_data,
            &owner,
            false,
        );
        let program_config_account = AccountInfo::new(
            &program_config_key,
            false,
            true,
            &mut program_config_lamports,
            &mut program_config_data,
            &owner,
            false,
        );

        try_transfer_program_authority(
            &[authority_account, program_config_account.clone()],
            TransferProgramAuthorityArgs {
                new_authority: new_authority.to_bytes(),
            },
        )
        .unwrap();

        assert_eq!(
            load_program_config(&program_config_account).authority(),
            new_authority
        );
    }

    #[test]
    fn rejects_non_authority_signer() {
        let authority = Pubkey::new_unique();
        let wrong_authority = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let (program_config_key, _) = ProgramConfig::find_address();
        let owner = crate::ID;
        let mut authority_lamports = 1;
        let mut authority_data = [];
        let mut program_config_lamports = 1;
        let mut program_config_data = program_config_account_data(authority);
        let authority_account = AccountInfo::new(
            &wrong_authority,
            true,
            false,
            &mut authority_lamports,
            &mut authority_data,
            &owner,
            false,
        );
        let program_config_account = AccountInfo::new(
            &program_config_key,
            false,
            true,
            &mut program_config_lamports,
            &mut program_config_data,
            &owner,
            false,
        );

        assert_eq!(
            try_transfer_program_authority(
                &[authority_account, program_config_account],
                TransferProgramAuthorityArgs {
                    new_authority: new_authority.to_bytes(),
                },
            ),
            Err(ProgramError::IllegalOwner)
        );
    }

    #[test]
    fn rejects_missing_authority_signature() {
        let authority = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let (program_config_key, _) = ProgramConfig::find_address();
        let owner = crate::ID;
        let mut authority_lamports = 1;
        let mut authority_data = [];
        let mut program_config_lamports = 1;
        let mut program_config_data = program_config_account_data(authority);
        let authority_account = AccountInfo::new(
            &authority,
            false,
            false,
            &mut authority_lamports,
            &mut authority_data,
            &owner,
            false,
        );
        let program_config_account = AccountInfo::new(
            &program_config_key,
            false,
            true,
            &mut program_config_lamports,
            &mut program_config_data,
            &owner,
            false,
        );

        assert_eq!(
            try_transfer_program_authority(
                &[authority_account, program_config_account],
                TransferProgramAuthorityArgs {
                    new_authority: new_authority.to_bytes(),
                },
            ),
            Err(ProgramError::MissingRequiredSignature)
        );
    }

    #[test]
    fn rejects_non_writable_program_config() {
        let authority = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let (program_config_key, _) = ProgramConfig::find_address();
        let owner = crate::ID;
        let mut authority_lamports = 1;
        let mut authority_data = [];
        let mut program_config_lamports = 1;
        let mut program_config_data = program_config_account_data(authority);
        let authority_account = AccountInfo::new(
            &authority,
            true,
            false,
            &mut authority_lamports,
            &mut authority_data,
            &owner,
            false,
        );
        let program_config_account = AccountInfo::new(
            &program_config_key,
            false,
            false,
            &mut program_config_lamports,
            &mut program_config_data,
            &owner,
            false,
        );

        assert_eq!(
            try_transfer_program_authority(
                &[authority_account, program_config_account],
                TransferProgramAuthorityArgs {
                    new_authority: new_authority.to_bytes(),
                },
            ),
            Err(ProgramError::InvalidAccountData)
        );
    }
}
