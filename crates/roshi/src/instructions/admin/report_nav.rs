use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_sysvar::{clock::Clock, Sysvar};
use wincode::serialize;

use crate::{
    instructions::{accounts::next_account, ReportNavArgs},
    state::{
        vault::{Role, Vault},
        Account,
    },
};
use roshi_interface::{error::RoshiError, math::nav_delta_within_bps};

const EMPTY_REPORT_HASH: [u8; 32] = [0; 32];

/// Implements [`crate::instructions::RoshiInstructionTag::ReportNav`].
///
/// # Accounts
///
/// 0. `[signer]` Vault NAV authority.
/// 1. `[writable]` Vault account receiving the accepted NAV report.
///
/// The NAV authority is trusted to report total portfolio NAV in base atoms.
/// Roshi enforces the vault's `max_change_bps` guardrail after the first
/// accepted report, then stores `total_assets`, `last_report_hash`, and
/// `last_update_ts`.
pub fn try_report_nav(accounts: &[AccountInfo], args: ReportNavArgs) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let nav_authority = next_account(accounts_iter)?;
    let vault_account = next_account(accounts_iter)?;
    if !vault_account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut vault = Account::load_as::<Vault>(vault_account)?;
    vault.verify_address(vault_account.key)?;
    vault.verify_role(Role::NavAuthority, nav_authority)?;

    if args.report_hash == EMPTY_REPORT_HASH {
        return Err(RoshiError::InvalidVaultState.into());
    }

    if vault.last_report_hash != EMPTY_REPORT_HASH
        && !nav_delta_within_bps(vault.total_assets, args.total_assets, vault.max_change_bps)?
    {
        return Err(RoshiError::InvalidVaultState.into());
    }

    vault.total_assets = args.total_assets;
    vault.last_report_hash = args.report_hash;
    vault.last_update_ts = Clock::get()?.unix_timestamp;
    vault.validate_state()?;

    let serialized =
        serialize(&Account::Vault(vault)).map_err(|_| ProgramError::InvalidAccountData)?;
    let mut data = vault_account.try_borrow_mut_data()?;
    if serialized.len() > data.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    data[..serialized.len()].copy_from_slice(&serialized);

    Ok(())
}
