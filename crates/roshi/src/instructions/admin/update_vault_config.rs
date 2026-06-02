use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;
use solana_pubkey::Pubkey;

use crate::{
    instructions::{
        accounts::{next_account, WritableVaultRoleContext},
        token, UpdateVaultConfigArgs,
    },
    state::vault::Role,
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstructionTag::UpdateVaultConfig`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account being reconfigured.
/// 2. `[]` New base fee collector token account.
///
/// Verifies the vault admin and atomically replaces mutable non-RBAC config:
/// fee collector, base oracle, default subaccounts, and fee settings. RBAC
/// authorities are intentionally handled by explicit
/// `Set*Authority` and transfer instructions. Pause flags are intentionally
/// handled by `SetPauseFlags`; access mode and Merkle root by `SetVaultAccess`.
/// The replacement config is validated by `validate_state` when the vault is
/// stored, so invalid fees or oracle config are rejected.
pub fn try_update_vault_config(
    accounts: &[AccountInfo],
    args: UpdateVaultConfigArgs,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let admin = next_account(accounts_iter)?;
    let vault_account = next_account(accounts_iter)?;
    let mut context = WritableVaultRoleContext::load(admin, vault_account, Role::Admin)?;

    let fee_collector = next_account(accounts_iter)?;
    let base_mint = Pubkey::from(context.vault().base_mint);
    if fee_collector.key != &Pubkey::from(args.fee_collector) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }
    token::verify_token_account_mint(fee_collector, &base_mint)?;

    let vault = context.vault_mut();
    vault.fee_collector = args.fee_collector;
    vault.deposit_sub_account = args.deposit_sub_account;
    vault.withdraw_sub_account = args.withdraw_sub_account;
    vault.base_oracle = args.base_oracle;
    vault.performance_fee_bps = args.performance_fee_bps;
    vault.withdrawal_buffer_bps = args.withdrawal_buffer_bps;
    context.store()
}
