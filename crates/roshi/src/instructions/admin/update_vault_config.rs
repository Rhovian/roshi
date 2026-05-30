use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::UpdateVaultConfigArgs;

/// Implements [`crate::instructions::RoshiInstruction::UpdateVaultConfig`].
///
/// # Accounts
///
/// Planned layout:
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account being reconfigured.
///
/// # Implementation
///
/// This handler is currently a stub. The intended implementation verifies the
/// vault admin and atomically replaces mutable role, base oracle, default
/// subaccount, fee, and NAV guardrail fields. Pause flags are intentionally handled by
/// `SetPauseFlags`.
pub fn try_update_vault_config(
    _accounts: &[AccountInfo],
    _args: UpdateVaultConfigArgs,
) -> ProgramResult {
    let _ = _args;
    Ok(())
}
