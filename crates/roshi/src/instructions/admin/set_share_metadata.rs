use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_error::{ProgramError, ProgramResult};
use solana_system_interface::program as system_program;

use crate::{
    instructions::{
        accounts::{next_account, require_writable},
        metaplex, SetShareMetadataArgs,
    },
    state::vault::{self, Role, Vault},
};

/// Implements [`crate::instructions::RoshiInstruction::SetShareMetadata`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Vault admin (pays metadata rent on first call).
/// 1. `[]` Vault.
/// 2. `[]` Share mint (`vault.share_mint`; the vault PDA is its mint
///    authority).
/// 3. `[writable]` Metadata PDA `["metadata", token_metadata_program,
///    share_mint]`.
/// 4. `[]` Metaplex Token Metadata program (vetted constant).
/// 5. `[]` System program.
///
/// Creates the share mint's Metaplex metadata on first call and updates it
/// thereafter (detected by metadata-account emptiness). The vault PDA signs
/// as mint authority and is recorded as the metadata update authority, so
/// renames go through this same admin instruction and nothing outside the
/// program can ever mutate it. Display only: if the CPI target is
/// unavailable the instruction fails loudly and nothing else is affected.
pub fn try_set_share_metadata(
    accounts: &[AccountInfo],
    args: SetShareMetadataArgs,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let admin = next_account(accounts_iter)?;
    if !admin.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let vault_account = next_account(accounts_iter)?;
    let vault = vault::load_checked(vault_account)?;
    vault::verify_role(&vault, Role::Admin, admin)?;

    let share_mint = next_account(accounts_iter)?;
    vault::verify_share_mint(&vault, share_mint)?;

    let metadata = next_account(accounts_iter)?;
    require_writable(metadata)?;
    let (expected_metadata, _) = metaplex::find_metadata_address(share_mint.key);
    if metadata.key != &expected_metadata {
        return Err(ProgramError::InvalidSeeds);
    }

    let token_metadata_program = next_account(accounts_iter)?;
    if token_metadata_program.key != &metaplex::TOKEN_METADATA_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let system_program_acc = next_account(accounts_iter)?;
    if system_program_acc.key != &system_program::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (instruction, account_infos): (Instruction, Vec<AccountInfo>) = if metadata.data_is_empty()
    {
        (
            Instruction {
                program_id: metaplex::TOKEN_METADATA_PROGRAM_ID,
                accounts: vec![
                    AccountMeta::new(*metadata.key, false),
                    AccountMeta::new_readonly(*share_mint.key, false),
                    AccountMeta::new_readonly(*vault_account.key, true), // mint authority
                    AccountMeta::new(*admin.key, true),                  // payer
                    AccountMeta::new_readonly(*vault_account.key, true), // update authority
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: metaplex::create_metadata_v3_data(&args.name, &args.symbol, &args.uri),
            },
            vec![
                metadata.clone(),
                share_mint.clone(),
                vault_account.clone(),
                admin.clone(),
                system_program_acc.clone(),
                token_metadata_program.clone(),
            ],
        )
    } else {
        (
            Instruction {
                program_id: metaplex::TOKEN_METADATA_PROGRAM_ID,
                accounts: vec![
                    AccountMeta::new(*metadata.key, false),
                    AccountMeta::new_readonly(*vault_account.key, true), // update authority
                ],
                data: metaplex::update_metadata_v2_data(&args.name, &args.symbol, &args.uri),
            },
            vec![
                metadata.clone(),
                vault_account.clone(),
                token_metadata_program.clone(),
            ],
        )
    };

    let tag = vault.tag_seed()?;
    let bump = [vault.bump];
    let signer_seeds: &[&[u8]] = &[Vault::SEED, tag, &vault.base_mint, &bump];
    invoke_signed(&instruction, &account_infos, &[signer_seeds])
}
