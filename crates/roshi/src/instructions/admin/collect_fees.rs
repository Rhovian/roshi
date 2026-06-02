use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use crate::{
    instructions::{
        accounts::next_account,
        token::{self, TOKEN_PROGRAM_ID},
        CollectFeesArgs,
    },
    state::{
        sub_account::VaultSubAccount,
        vault::{Role, Vault},
        Account,
    },
};
use roshi_interface::error::RoshiError;

/// Implements [`crate::instructions::RoshiInstruction::CollectFees`].
///
/// # Accounts
///
/// 0. `[signer]` Vault admin.
/// 1. `[writable]` Vault account with accrued `fees_payable`.
/// 2. `[]` Vault subaccount PDA for `args.sub_account`.
/// 3. `[writable]` Vault subaccount base custody token account.
/// 4. `[writable]` Configured base fee collector token account.
/// 5. `[]` SPL Token program.
///
/// Fees are accrued during NAV reporting, so collection only settles an
/// existing payable and does not change `total_assets`.
pub fn try_collect_fees(accounts: &[AccountInfo], args: CollectFeesArgs) -> ProgramResult {
    if args.amount == 0 {
        return Err(RoshiError::ZeroOutput.into());
    }

    let accounts_iter = &mut accounts.iter();

    let admin = next_account(accounts_iter)?;
    let vault_account = next_account(accounts_iter)?;
    require_writable(vault_account)?;

    let mut vault = Account::load_as::<Vault>(vault_account)?;
    vault.verify_address(vault_account.key)?;
    vault.verify_role(Role::Admin, admin)?;

    if args.amount > vault.fees_payable {
        return Err(RoshiError::InvalidVaultState.into());
    }

    let fee_sub_account = next_account(accounts_iter)?;
    let sub_account_bump =
        VaultSubAccount::verify_account(vault_account.key, args.sub_account, fee_sub_account)?;

    let custody = next_account(accounts_iter)?;
    require_writable(custody)?;
    let base_mint = Pubkey::from(vault.base_mint);
    token::verify_token_account_mint_and_owner(custody, &base_mint, fee_sub_account.key)?;

    let fee_collector = next_account(accounts_iter)?;
    require_writable(fee_collector)?;
    if fee_collector.key != &Pubkey::from(vault.fee_collector) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }
    token::verify_token_account_mint(fee_collector, &base_mint)?;

    let token_program = next_account(accounts_iter)?;
    if token_program.key != &TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let sub_account_bump = [sub_account_bump];
    let fee_sub_account_index = [args.sub_account];
    let signer_seeds: &[&[u8]] = &[
        VaultSubAccount::SEED,
        vault_account.key.as_ref(),
        &fee_sub_account_index,
        &sub_account_bump,
    ];
    token::transfer_signed(
        token_program,
        custody,
        fee_collector,
        fee_sub_account,
        args.amount,
        signer_seeds,
    )?;

    vault.fees_payable = vault
        .fees_payable
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
