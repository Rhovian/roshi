use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use wincode::serialize;

use crate::{
    instructions::accounts::next_account,
    state::{
        vault::{Role, Vault},
        Account,
    },
};

pub(super) fn update_vault_as_admin(
    accounts: &[AccountInfo],
    update: impl FnOnce(&mut Vault) -> ProgramResult,
) -> ProgramResult {
    let mut accounts_iter = accounts.iter();
    let admin = next_account(&mut accounts_iter)?;
    let vault_account = next_account(&mut accounts_iter)?;

    if !vault_account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut vault = Account::load_as::<Vault>(vault_account)?;
    vault.verify_address(vault_account.key)?;
    vault.verify_role(Role::Admin, admin)?;

    update(&mut vault)?;

    let serialized =
        serialize(&Account::Vault(vault)).map_err(|_| ProgramError::InvalidAccountData)?;
    let mut data = vault_account.try_borrow_mut_data()?;
    if serialized.len() > data.len() {
        return Err(ProgramError::InvalidAccountData);
    }

    data[..serialized.len()].copy_from_slice(&serialized);

    Ok(())
}
