use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use wincode::serialize;

use crate::{
    instructions::{accounts::InitializeVaultContext, InitializeVaultArgs},
    state::{vault::Vault, Account},
};

/// Implements [`crate::instructions::RoshiInstruction::InitializeVault`].
///
/// # Accounts
///
/// Account layout:
/// 0. `[signer]` Program authority stored in the program config account.
/// 1. `[]` Program config PDA derived from `ProgramConfig::SEED`.
/// 2. `[signer, writable]` Payer funding vault creation.
/// 3. `[writable]` Vault PDA derived from `[b"vault", tag, base_mint]`.
/// 4. `[]` Base mint (decimals must equal `base_decimals`).
/// 5. `[]` Share mint (must have 9 decimals and the vault PDA as mint authority).
/// 6. `[]` Base fee collector token account.
/// 7. `[]` System program.
///
/// # Implementation
///
/// Verifies the program authority gate, validates the vault tag and PDA seeds,
/// validates the base and share mint accounts, creates the vault account with
/// rent-exempt lamports, records configured role authorities, base-asset oracle
/// config, and default subaccounts, initializes fee and access config, clears
/// pause flags, and starts accounting from an empty-share, empty-asset state.
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
        args.private,
        args.access_merkle_root,
        accounts.vault_bump(),
    )?;
    accounts.verify_mints(&args)?;

    let serialized =
        serialize(&Account::Vault(vault)).map_err(|_| ProgramError::InvalidAccountData)?;

    if serialized.len() > Vault::SPACE {
        return Err(ProgramError::InvalidAccountData);
    }

    accounts.create_vault_account()?;
    accounts.store_vault(&serialized)
}
