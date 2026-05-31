use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use crate::{
    error::RoshiError,
    instructions::InitializeVaultArgs,
    state::{program_config::ProgramConfig, vault::Vault},
};

/// Implements [`crate::instructions::RoshiInstruction::InitializeVault`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Program authority stored in the program config account.
/// 1. `[]` Program config PDA derived from `ProgramConfig::SEED`.
/// 2. `[signer, writable]` Payer funding vault creation.
/// 3. `[writable]` Vault PDA derived from `[b"vault", tag, base_mint]`.
/// 4. `[]` System program.
///
/// # Implementation
///
/// Verifies the program authority gate. The rest of this handler is currently
/// a stub: the intended implementation creates the vault account, records role
/// authorities, base-asset oracle config, and default subaccounts, initializes
/// fee, access, and NAV guardrail config, clears pause flags, and starts
/// accounting from an empty-share, empty-asset state.
pub fn try_initialize_vault(accounts: &[AccountInfo], args: InitializeVaultArgs) -> ProgramResult {
    let mut accounts_iter = accounts.iter();
    let program_authority = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let program_config = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let _payer = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let vault_account = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    ProgramConfig::verify_authority(program_config, program_authority)?;

    if !vault_account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    let tag_len = usize::from(args.tag_len);
    let tag = args.tag.get(..tag_len).ok_or(RoshiError::InvalidVaultTag)?;
    let base_mint = Pubkey::from(args.base_mint);
    let (expected_vault_key, _) = Vault::find_address(tag, &base_mint)?;
    if vault_account.key != &expected_vault_key {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(())
}
