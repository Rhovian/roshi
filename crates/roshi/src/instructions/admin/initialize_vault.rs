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
/// 5. `[writable]` Share mint PDA derived from `[b"share_mint", vault]`.
/// 6. `[]` Base treasury token account.
/// 7. `[]` System program.
/// 8. `[]` SPL Token program.
///
/// # Implementation
///
/// Verifies the program authority gate, validates the vault tag and PDA seeds,
/// validates the base mint and treasury, creates the vault account and
/// share mint PDA with rent-exempt lamports, initializes the share mint with
/// fixed 9 decimals and the vault PDA as mint authority, records configured
/// role authorities, base-asset oracle config, and default subaccounts,
/// initializes fee and access config, clears pause flags, and starts accounting
/// from an empty-share, empty-asset state.
pub fn try_initialize_vault(accounts: &[AccountInfo], args: InitializeVaultArgs) -> ProgramResult {
    let accounts = InitializeVaultContext::load(accounts, &args)?;
    let tag = Vault::unpack_tag(&args.tag, args.tag_len)?;
    let vault = Vault::new(
        tag,
        args.admin,
        args.strategist,
        args.swap_authority,
        args.nav_authority,
        args.withdrawal_authority,
        args.base_mint,
        accounts.share_mint(),
        args.base_decimals,
        args.base_oracle,
        args.deposit_sub_account,
        args.withdraw_sub_account,
        args.treasury,
        args.performance_fee_bps,
        args.withdrawal_buffer_bps,
        args.controls,
        args.private,
        args.access_merkle_root,
        accounts.vault_bump(),
    )?;
    accounts.verify_external_token_accounts(&args)?;

    let serialized =
        serialize(&Account::Vault(vault)).map_err(|_| ProgramError::InvalidAccountData)?;

    if serialized.len() > Vault::SPACE {
        return Err(ProgramError::InvalidAccountData);
    }

    accounts.create_vault_account()?;
    accounts.create_share_mint()?;
    accounts.store_vault(&serialized)
}
