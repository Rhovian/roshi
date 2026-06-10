use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use crate::{
    instructions::{
        accounts::{next_account, require_writable},
        token, InvestExternalArgs,
    },
    state::{
        sub_account::VaultSubAccount,
        vault::{self, Role},
        Account,
    },
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::InvestExternal`].
///
/// # Accounts
///
/// 0. `[signer]` Vault strategist.
/// 1. `[writable]` Vault account whose base assets are being deployed externally.
/// 2. `[]` Vault subaccount PDA for `args.sub_account`.
/// 3. `[writable]` Vault subaccount base custody token account.
/// 4. `[writable]` External base token account receiving the investment cash.
/// 5. `[]` SPL Token program.
///
/// Moves base assets out to an external investment account without changing
/// `total_assets`: the vault still owns the economic asset, only its custody
/// location changes. The deployed amount is added to `external_assets`, while
/// `ReturnExternal` moves cash back and decrements that tracked amount.
pub fn try_invest_external(accounts: &[AccountInfo], args: InvestExternalArgs) -> ProgramResult {
    if args.amount == 0 {
        return Err(RoshiError::ZeroOutput.into());
    }

    let accounts_iter = &mut accounts.iter();

    let strategist = next_account(accounts_iter)?;
    let vault_account = next_account(accounts_iter)?;
    require_writable(vault_account)?;
    let mut vault = vault::load_checked(vault_account)?;
    vault::verify_role(&vault, Role::Strategist, strategist)?;
    vault.verify_manage_enabled()?;
    if !vault.external_enabled()? {
        return Err(RoshiError::ExternalDisabled.into());
    }

    let sub_account = next_account(accounts_iter)?;
    let sub_account_bump =
        VaultSubAccount::verify_account(vault_account.key, args.sub_account, sub_account)?;

    let custody = next_account(accounts_iter)?;
    require_writable(custody)?;
    let base_mint = Pubkey::from(vault.base_mint);
    token::verify_token_account_mint_and_owner(custody, &base_mint, sub_account.key)?;

    let external_account = next_account(accounts_iter)?;
    require_writable(external_account)?;
    token::verify_token_account_mint(external_account, &base_mint)?;

    let token_program = next_account(accounts_iter)?;
    token::verify_token_program_for(token_program, custody)?;
    token::verify_token_program_for(token_program, external_account)?;

    let sub_account_bump = [sub_account_bump];
    let sub_account_index = [args.sub_account];
    let signer_seeds: &[&[u8]] = &[
        VaultSubAccount::SEED,
        vault_account.key.as_ref(),
        &sub_account_index,
        &sub_account_bump,
    ];
    token::transfer_signed(
        token_program,
        custody,
        external_account,
        sub_account,
        args.amount,
        signer_seeds,
    )?;

    vault.external_assets = vault
        .external_assets
        .checked_add(args.amount)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_amount() {
        assert_eq!(
            try_invest_external(
                &[],
                InvestExternalArgs {
                    sub_account: 0,
                    amount: 0,
                },
            ),
            Err(ProgramError::from(RoshiError::ZeroOutput))
        );
    }
}
