use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use crate::{instructions::InitializeVaultArgs, state::program_config::ProgramConfig};

/// Implements [`crate::instructions::RoshiInstruction::InitializeVault`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Program authority stored in the program config account.
/// 1. `[]` Program config PDA derived from `ProgramConfig::SEED`.
/// 2. `[signer, writable]` Payer funding vault creation.
/// 3. `[writable]` Vault PDA derived from `(admin, base_mint)`.
/// 4. `[]` System program.
///
/// # Implementation
///
/// Verifies the program authority gate. The rest of this handler is currently
/// a stub: the intended implementation creates the vault account, records role
/// authorities and default subaccounts, initializes fee and NAV guardrail
/// config, clears pause flags, and starts accounting from an empty-share,
/// empty-asset state.
pub fn try_initialize_vault(accounts: &[AccountInfo], _args: InitializeVaultArgs) -> ProgramResult {
    let mut accounts_iter = accounts.iter();
    let program_authority = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let program_config = accounts_iter
        .next()
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    ProgramConfig::verify_authority(program_config, program_authority)?;

    let _ = _args;
    Ok(())
}
