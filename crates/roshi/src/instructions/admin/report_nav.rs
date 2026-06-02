use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_sysvar::{clock::Clock, Sysvar};
use wincode::serialize;

use crate::{
    instructions::{accounts::next_account, token, ReportNavArgs},
    state::{
        vault::{Role, Vault},
        Account,
    },
};
use roshi_interface::{error::RoshiError, math::performance_fee_for_nav};

const EMPTY_REPORT_HASH: [u8; 32] = [0; 32];

/// Implements [`crate::instructions::RoshiInstructionTag::ReportNav`].
///
/// # Accounts
///
/// 0. `[signer]` Vault NAV authority.
/// 1. `[writable]` Vault account receiving the accepted NAV report.
/// 2. `[]` SPL share mint.
///
/// The NAV authority is trusted to report gross total portfolio NAV in base
/// atoms. The gross report must include assets reserved or owed for pending
/// withdrawals and unpaid fees. Roshi subtracts existing liabilities, accrues
/// any performance fee into `fees_payable`, stores net `total_assets`, and
/// records the report commitment and timestamp.
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

    let share_mint = next_account(accounts_iter)?;
    if share_mint.key != &Pubkey::from(vault.share_mint) {
        return Err(RoshiError::InvalidMintAccount.into());
    }
    let share_supply = token::mint_supply(share_mint)?;
    let economic_share_supply = share_supply
        .checked_add(vault.requested_withdrawal_shares)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;

    if args.report_hash == EMPTY_REPORT_HASH {
        return Err(RoshiError::InvalidVaultState.into());
    }

    let fee_base_assets = args
        .total_assets
        .checked_sub(vault.fees_payable)
        .and_then(|assets| assets.checked_sub(vault.pending_withdrawal_assets))
        .ok_or(ProgramError::from(RoshiError::InvalidVaultState))?;
    let (fee_assets, net_total_assets, high_watermark) = performance_fee_for_nav(
        fee_base_assets,
        economic_share_supply,
        vault.high_watermark,
        vault.performance_fee_bps,
    )?;

    vault.fees_payable = vault
        .fees_payable
        .checked_add(fee_assets)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
    vault.total_assets = net_total_assets;
    vault.high_watermark = high_watermark;
    vault.report_epoch = vault
        .report_epoch
        .checked_add(1)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
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
