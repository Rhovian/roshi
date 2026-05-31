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
/// vault admin and atomically replaces mutable non-RBAC config: fee collector,
/// base oracle, default subaccounts, fee settings, and NAV guardrails. RBAC
/// authorities are intentionally handled by explicit `Set*Authority` and
/// transfer instructions. Pause flags are intentionally handled by
/// `SetPauseFlags`; access mode and Merkle root are intentionally handled by
/// `SetVaultAccess`.
pub fn try_update_vault_config(
    _accounts: &[AccountInfo],
    _args: UpdateVaultConfigArgs,
) -> ProgramResult {
    let _ = _args;
    Ok(())
}
