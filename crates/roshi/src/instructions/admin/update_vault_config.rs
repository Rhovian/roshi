use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{accounts::update_writable_vault_as_admin, UpdateVaultConfigArgs};

/// Implements [`crate::instructions::RoshiInstructionTag::UpdateVaultConfig`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account being reconfigured.
///
/// Verifies the vault admin and atomically replaces mutable non-RBAC config:
/// fee collector, base oracle, default subaccounts, fee settings, and NAV change
/// guardrails. RBAC authorities are intentionally handled by explicit
/// `Set*Authority` and transfer instructions. Pause flags are intentionally
/// handled by `SetPauseFlags`; access mode and Merkle root by `SetVaultAccess`.
/// The replacement config is validated by `validate_state` when the vault is
/// stored, so invalid fees, guardrails, or oracle config are rejected.
pub fn try_update_vault_config(
    accounts: &[AccountInfo],
    args: UpdateVaultConfigArgs,
) -> ProgramResult {
    update_writable_vault_as_admin(accounts, |vault| {
        vault.fee_collector = args.fee_collector;
        vault.deposit_sub_account = args.deposit_sub_account;
        vault.withdraw_sub_account = args.withdraw_sub_account;
        vault.base_oracle = args.base_oracle;
        vault.performance_fee_bps = args.performance_fee_bps;
        vault.withdrawal_buffer_bps = args.withdrawal_buffer_bps;
        vault.max_change_bps = args.max_change_bps;
        Ok(())
    })
}
