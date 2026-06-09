use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_sysvar::{clock::Clock, Sysvar};
use wincode::serialize;

use crate::{
    instructions::{accounts::next_account, token, ReportNavArgs},
    state::{
        sub_account::VaultSubAccount,
        vault::{Role, Vault, VaultExt},
        Account,
    },
};
use roshi_interface::{error::RoshiError, math::performance_fee_for_nav};

const EMPTY_REPORT_HASH: [u8; 32] = [0; 32];

/// Implements [`crate::instructions::RoshiInstruction::ReportNav`].
///
/// # Accounts
///
/// 0. `[signer]` Vault NAV authority.
/// 1. `[writable]` Vault account receiving the accepted NAV report.
/// 2. `[]` SPL share mint.
/// 3. `[]` Base-asset token program (owns the custody ATAs).
/// 4. `[]` Deposit sub-account base ATA.
/// 5. `[]` Withdraw sub-account base ATA.
///
/// The NAV authority reports `external_value` — the marked base-atom value of
/// everything outside idle base custody (venue positions, non-base idle). The
/// program reads **idle base** on-chain from the vault's deposit and withdraw
/// sub-account base ATAs (so it cannot be misreported), forms gross NAV =
/// idle + `external_value`, then subtracts existing liabilities, accrues any
/// performance fee into `fees_payable`, stores net `total_assets`, and records
/// the report commitment and timestamp.
pub fn try_report_nav(accounts: &[AccountInfo], args: ReportNavArgs) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let nav_authority = next_account(accounts_iter)?;
    let vault_account = next_account(accounts_iter)?;
    if !vault_account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut vault = Vault::load_checked(vault_account)?;
    vault.verify_role(Role::NavAuthority, nav_authority)?;

    let share_mint = next_account(accounts_iter)?;
    vault.verify_share_mint(share_mint)?;
    let share_supply = token::mint_supply(share_mint)?;
    let economic_share_supply = vault.economic_share_supply(share_supply)?;

    if args.report_hash == EMPTY_REPORT_HASH {
        return Err(RoshiError::InvalidVaultState.into());
    }

    // Idle base: the vault's own base-mint balance, read live from its deposit and
    // withdraw sub-account ATAs. Pinned to the canonical ATAs so the authority
    // cannot substitute a sandbagged account. Gross NAV = idle + external_value.
    let base_token_program = next_account(accounts_iter)?;
    token::verify_token_program(base_token_program)?;
    let deposit_custody = next_account(accounts_iter)?;
    let withdraw_custody = next_account(accounts_iter)?;

    let base_mint = Pubkey::from(vault.base_mint);
    let deposit_ata = expect_base_custody(
        vault_account.key,
        vault.deposit_sub_account,
        &base_mint,
        base_token_program.key,
        deposit_custody,
    )?;
    let withdraw_ata = expect_base_custody(
        vault_account.key,
        vault.withdraw_sub_account,
        &base_mint,
        base_token_program.key,
        withdraw_custody,
    )?;

    let mut idle = custody_amount(deposit_custody)?;
    // Distinct sub-accounts ⇒ distinct ATAs. If a vault points both roles at the
    // same sub-account the ATAs coincide; count the balance once.
    if withdraw_ata != deposit_ata {
        idle = idle
            .checked_add(custody_amount(withdraw_custody)?)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
    }
    let gross_total_assets = idle
        .checked_add(args.external_value)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;

    let fee_base_assets = gross_total_assets
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

/// Verify `custody` is the canonical base ATA of the vault's `sub_account_index`
/// sub-account, returning that ATA address.
fn expect_base_custody(
    vault_key: &Pubkey,
    sub_account_index: u8,
    base_mint: &Pubkey,
    base_token_program: &Pubkey,
    custody: &AccountInfo,
) -> Result<Pubkey, ProgramError> {
    let (sub_account, _) = VaultSubAccount::find_address(vault_key, sub_account_index);
    let expected = token::associated_token_address(&sub_account, base_mint, base_token_program);
    if custody.key != &expected {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(expected)
}

/// Base balance held in a (pinned) custody ATA, or `0` if it isn't created yet.
fn custody_amount(custody: &AccountInfo) -> Result<u64, ProgramError> {
    if custody.data_is_empty() {
        return Ok(0);
    }

    token::token_amount(custody)
}
