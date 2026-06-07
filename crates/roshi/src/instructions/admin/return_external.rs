use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use crate::{
    instructions::{
        accounts::next_account,
        token::{self, TOKEN_PROGRAM_ID},
        ReturnExternalArgs,
    },
    state::{
        sub_account::VaultSubAccount,
        vault::{Role, Vault},
        Account,
    },
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::ReturnExternal`].
///
/// # Accounts
///
/// 0. `[signer]` Vault strategist.
/// 1. `[signer]` External token account authority.
/// 2. `[writable]` Vault account whose external asset accounting is reduced.
/// 3. `[]` Vault subaccount PDA for `args.sub_account`.
/// 4. `[writable]` External base token account returning cash.
/// 5. `[writable]` Vault subaccount base custody token account.
/// 6. `[]` SPL Token program.
///
/// Moves base assets back into vault custody and decrements `external_assets`.
/// This is the mirror of `InvestExternal`: NAV reports stay valuation-only,
/// while actual external cash movement is tracked by explicit token transfers.
pub fn try_return_external(accounts: &[AccountInfo], args: ReturnExternalArgs) -> ProgramResult {
    if args.amount == 0 {
        return Err(RoshiError::ZeroOutput.into());
    }

    let accounts_iter = &mut accounts.iter();

    let strategist = next_account(accounts_iter)?;
    let external_authority = next_account(accounts_iter)?;
    if !external_authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let vault_account = next_account(accounts_iter)?;
    require_writable(vault_account)?;
    let mut vault = Account::load_as::<Vault>(vault_account)?;
    vault.verify_address(vault_account.key)?;
    vault.verify_role(Role::Strategist, strategist)?;
    vault.verify_manage_enabled()?;
    if !vault.external_enabled()? {
        return Err(RoshiError::ExternalDisabled.into());
    }
    if args.amount > vault.external_assets {
        return Err(RoshiError::InvalidVaultState.into());
    }

    let sub_account = next_account(accounts_iter)?;
    VaultSubAccount::verify_account(vault_account.key, args.sub_account, sub_account)?;

    let external_account = next_account(accounts_iter)?;
    require_writable(external_account)?;
    let base_mint = Pubkey::from(vault.base_mint);
    token::verify_token_account_mint_and_owner(
        external_account,
        &base_mint,
        external_authority.key,
    )?;

    let custody = next_account(accounts_iter)?;
    require_writable(custody)?;
    token::verify_token_account_mint_and_owner(custody, &base_mint, sub_account.key)?;

    let token_program = next_account(accounts_iter)?;
    if token_program.key != &TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    token::transfer(
        token_program,
        external_account,
        custody,
        external_authority,
        args.amount,
    )?;

    vault.external_assets = vault
        .external_assets
        .checked_sub(args.amount)
        .ok_or(ProgramError::from(RoshiError::Overflow))?;
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

fn require_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_amount() {
        assert_eq!(
            try_return_external(
                &[],
                ReturnExternalArgs {
                    sub_account: 0,
                    amount: 0,
                },
            ),
            Err(ProgramError::from(RoshiError::ZeroOutput))
        );
    }
}
