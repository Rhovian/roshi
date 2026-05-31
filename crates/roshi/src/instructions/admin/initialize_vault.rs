use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use wincode::serialize;

use crate::{
    instructions::{accounts::InitializeVaultContext, InitializeVaultArgs},
    state::{vault::Vault, Account},
};

/// Implements [`crate::instructions::RoshiInstructionTag::InitializeVault`].
///
/// # Accounts
///
/// Account layout:
/// 0. `[signer]` Program authority stored in the program config account.
/// 1. `[]` Program config PDA derived from `ProgramConfig::SEED`.
/// 2. `[signer, writable]` Payer funding vault creation.
/// 3. `[writable]` Vault PDA derived from `[b"vault", tag, base_mint]`.
/// 4. `[]` System program.
///
/// # Implementation
///
/// Verifies the program authority gate, validates the vault tag and PDA seeds,
/// creates the vault account with rent-exempt lamports, records configured
/// role authorities, base-asset oracle config, and default subaccounts,
/// initializes fee, access, and NAV guardrail config, clears pause flags, and
/// starts accounting from an empty-share, empty-asset state.
pub fn try_initialize_vault(accounts: &[AccountInfo], args: InitializeVaultArgs) -> ProgramResult {
    let accounts = InitializeVaultContext::load(accounts, &args)?;
    let tag = Vault::unpack_tag(&args.tag, args.tag_len)?;
    let vault = Vault::new(
        tag,
        args.admin,
        args.strategist,
        args.nav_authority,
        args.withdrawal_authority,
        args.base_mint,
        args.share_mint,
        args.base_decimals,
        args.base_oracle,
        args.deposit_sub_account,
        args.withdraw_sub_account,
        args.fee_collector,
        args.performance_fee_bps,
        args.withdrawal_buffer_bps,
        args.max_change_bps,
        args.min_update_interval,
        args.private,
        args.access_merkle_root,
        accounts.vault_bump(),
    )?;
    let serialized =
        serialize(&Account::Vault(vault)).map_err(|_| ProgramError::InvalidAccountData)?;

    if serialized.len() > Vault::SPACE {
        return Err(ProgramError::InvalidAccountData);
    }

    accounts.create_vault_account()?;
    accounts.store_vault(&serialized)
}
