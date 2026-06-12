use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_system_interface::instruction::create_account;
use solana_sysvar::{rent::Rent, Sysvar};
use wincode::serialize;

use super::{
    shared::{
        close_account, next_account, require_system_program, require_uninitialized_account,
        require_writable, require_writable_signer,
    },
    vault::VaultRoleContext,
};
use crate::{
    instructions::token,
    state::{external_destination::ExternalDestination, vault::Role, Account},
};

/// Loads `[admin signer+writable, vault, destination token account,
/// external_destination (writable, uninitialized), system program]`, verifies
/// the vault admin, validates the destination as a base-mint token account,
/// and binds the registration account to the PDA for `(vault, destination)`.
pub(crate) struct RegisterExternalDestinationContext<'a, 'info> {
    admin: &'a AccountInfo<'info>,
    external_destination: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    vault_key: Pubkey,
    token_account: Pubkey,
    bump: u8,
}

impl<'a, 'info> RegisterExternalDestinationContext<'a, 'info> {
    pub(crate) fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let admin = next_account(accounts_iter)?;
        require_writable_signer(admin)?;
        let vault_account = next_account(accounts_iter)?;
        let destination = next_account(accounts_iter)?;
        let external_destination = next_account(accounts_iter)?;
        require_uninitialized_account(external_destination)?;
        let system_program_acc = next_account(accounts_iter)?;
        require_system_program(system_program_acc)?;

        let context = VaultRoleContext::load(admin, vault_account, Role::Admin)?;
        let vault_key = context.vault_key();

        // `invest_external` only ever moves base out, so a destination is
        // authorized as a base-mint token account.
        let base_mint = Pubkey::from(context.vault().base_mint);
        token::verify_token_account_mint(destination, &base_mint)?;

        let (expected_key, bump) = ExternalDestination::find_address(&vault_key, destination.key);
        if external_destination.key != &expected_key {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Self {
            admin,
            external_destination,
            system_program_acc,
            vault_key,
            token_account: *destination.key,
            bump,
        })
    }

    /// Creates the rent-exempt registration PDA (funded by the admin) and
    /// stores the destination record.
    pub(crate) fn create_and_store(&self) -> ProgramResult {
        let destination = ExternalDestination::new(
            self.vault_key.to_bytes(),
            self.token_account.to_bytes(),
            self.bump,
        );

        let rent_exemption_lamports = Rent::get()?.minimum_balance(ExternalDestination::SPACE);
        let create_account_ix = create_account(
            self.admin.key,
            self.external_destination.key,
            rent_exemption_lamports,
            ExternalDestination::SPACE as u64,
            &crate::ID,
        );
        let account_infos = [
            self.admin.clone(),
            self.external_destination.clone(),
            self.system_program_acc.clone(),
        ];
        let bump = [self.bump];

        invoke_signed(
            &create_account_ix,
            &account_infos,
            &[&[
                ExternalDestination::SEED,
                self.vault_key.as_ref(),
                self.token_account.as_ref(),
                &bump,
            ]],
        )?;

        let serialized = serialize(&Account::ExternalDestination(destination))
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if serialized.len() > ExternalDestination::SPACE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut data = self.external_destination.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}

/// Loads `[admin signer+writable, vault, external_destination (writable)]`,
/// verifies the vault admin and that the registration is the PDA for
/// `(vault, recorded token account)`, then closes it, refunding rent to the
/// admin.
pub(crate) fn close_external_destination_as_admin(accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let admin = next_account(accounts_iter)?;
    require_writable_signer(admin)?;
    let vault_account = next_account(accounts_iter)?;
    let destination_account = next_account(accounts_iter)?;
    require_writable(destination_account)?;

    let context = VaultRoleContext::load(admin, vault_account, Role::Admin)?;
    let vault_key = context.vault_key();

    let destination = Account::load_as::<ExternalDestination>(destination_account)?;
    let (expected_key, expected_bump) =
        ExternalDestination::find_address(&vault_key, &Pubkey::from(destination.token_account));
    if destination_account.key != &expected_key || destination.bump != expected_bump {
        return Err(ProgramError::InvalidSeeds);
    }

    close_account(destination_account, admin)
}
