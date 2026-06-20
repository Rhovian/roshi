use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use wincode::serialize;

use crate::{
    instructions::{
        accounts::{next_account, require_writable, VaultRoleContext},
        AdminSetFlashFeeRateArgs, StrategistLowerFlashFeeRateArgs,
    },
    state::{action::Action, vault::Role, Account},
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::AdminSetFlashFeeRate`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[]` Vault that owns the action.
/// 2. `[writable]` Action PDA whose `FlashApprove` flash-fee rate is updated.
///
/// Sets the committed `(fee_num, fee_den)` to any value. **Raising** the rate
/// expands the strategist's delegate cap and, with the unpinned borrow amount,
/// is a theft lever (#22) — so it is admin-only. Handles the lender raising its
/// flash fee (a stale-low committed rate reverts entries fail-safe until reset).
pub fn try_admin_set_flash_fee_rate(
    accounts: &[AccountInfo],
    args: AdminSetFlashFeeRateArgs,
) -> ProgramResult {
    update_action_flash_fee(accounts, Role::Admin, |_action| {
        Ok((args.fee_num, args.fee_den))
    })
}

/// Implements [`crate::instructions::RoshiInstruction::StrategistLowerFlashFeeRate`].
///
/// # Accounts
///
/// 0. `[signer]` Vault strategist.
/// 1. `[]` Vault that owns the action.
/// 2. `[writable]` Action PDA whose `FlashApprove` flash-fee rate is lowered.
///
/// Sets the committed rate to a value **strictly below** the current one.
/// Lowering only shrinks the delegate cap, which can at worst make `flash_repay`
/// revert (fail-safe) — never skim — so the strategist may do it (#22), to track
/// the lender lowering its flash fee without an admin round-trip.
pub fn try_strategist_lower_flash_fee_rate(
    accounts: &[AccountInfo],
    args: StrategistLowerFlashFeeRateArgs,
) -> ProgramResult {
    update_action_flash_fee(accounts, Role::Strategist, |action| {
        if !rate_strictly_lower(args.fee_num, args.fee_den, action.fee_num, action.fee_den) {
            return Err(RoshiError::FlashFeeRateNotLower.into());
        }
        Ok((args.fee_num, args.fee_den))
    })
}

/// `[authority signer, vault, action writable]`: verify the caller's `role` on
/// the vault, load the action and bind it to that vault's PDA, derive the new
/// rate from `update`, and write it back in place. The rate is not part of the
/// action hash, so the PDA is unchanged.
fn update_action_flash_fee(
    accounts: &[AccountInfo],
    role: Role,
    update: impl FnOnce(&Action) -> Result<(u64, u64), ProgramError>,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let authority = next_account(accounts_iter)?;
    let vault_account = next_account(accounts_iter)?;
    let action_account = next_account(accounts_iter)?;
    require_writable(action_account)?;

    let context = VaultRoleContext::load(authority, vault_account, role)?;
    let vault_key = context.vault_key();

    let mut action = Account::load_as::<Action>(action_account)?;
    action.verify_for_vault(&vault_key, action_account.key)?;

    let (fee_num, fee_den) = update(&action)?;
    action.fee_num = fee_num;
    action.fee_den = fee_den;

    let serialized =
        serialize(&Account::Action(action)).map_err(|_| ProgramError::InvalidAccountData)?;
    let mut data = action_account.try_borrow_mut_data()?;
    if serialized.len() > data.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    data[..serialized.len()].copy_from_slice(&serialized);

    Ok(())
}

/// `new_num/new_den < cur_num/cur_den`, by cross-multiplication (no division).
/// A zero numerator is a zero rate regardless of denominator; this keeps a
/// `den == 0` operand from poisoning the comparison and reproduces "lower than
/// any positive rate, not lower than zero". `u64 * u64` cannot overflow `u128`.
fn rate_strictly_lower(new_num: u64, new_den: u64, cur_num: u64, cur_den: u64) -> bool {
    if new_num == 0 {
        return cur_num != 0;
    }
    if cur_num == 0 {
        return false;
    }
    u128::from(new_num) * u128::from(cur_den) < u128::from(cur_num) * u128::from(new_den)
}

#[cfg(test)]
mod tests {
    use super::rate_strictly_lower;

    #[test]
    fn strictly_lower_compares_rates_not_components() {
        // 0.00001 < 0.0001 even with different denominators.
        assert!(rate_strictly_lower(1, 100_000, 1, 10_000));
        assert!(!rate_strictly_lower(1, 10_000, 1, 100_000));
        // Equal rates in different forms are not strictly lower.
        assert!(!rate_strictly_lower(2, 20_000, 1, 10_000));
        // Zero is below any positive rate, but nothing is below zero.
        assert!(rate_strictly_lower(0, 1, 1, 10_000));
        assert!(!rate_strictly_lower(0, 1, 0, 1));
        assert!(!rate_strictly_lower(1, 10_000, 0, 1));
        // A degenerate den==0 positive "rate" can't satisfy strictly-lower.
        assert!(!rate_strictly_lower(1, 0, 1, 10_000));
    }
}
