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
use crate::state::{
    action::{validate_ops, Action, ActionScope, Ops},
    vault::Role,
    Account,
};

/// Loads `[admin signer+writable, vault, action (writable, uninitialized),
/// system program]`, verifies the vault admin, and binds the action account to
/// the PDA for `action_hash`.
pub(crate) struct AuthorizeActionContext<'a, 'info> {
    admin: &'a AccountInfo<'info>,
    action: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    vault_key: Pubkey,
    action_bump: u8,
}

impl<'a, 'info> AuthorizeActionContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        action_hash: &[u8; 32],
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let admin = next_account(accounts_iter)?;
        require_writable_signer(admin)?;
        let vault_account = next_account(accounts_iter)?;
        let action = next_account(accounts_iter)?;
        require_uninitialized_account(action)?;
        let system_program_acc = next_account(accounts_iter)?;
        require_system_program(system_program_acc)?;

        let context = VaultRoleContext::load(admin, vault_account, Role::Admin)?;
        let vault_key = context.vault_key();

        let (expected_action_key, action_bump) = Action::find_address(&vault_key, action_hash);
        if action.key != &expected_action_key {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Self {
            admin,
            action,
            system_program_acc,
            vault_key,
            action_bump,
        })
    }

    /// Creates the rent-exempt Action PDA (funded by the admin) and stores the
    /// vault scope, approved `action_hash`, `ops`, and bump.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_and_store(
        &self,
        action_hash: [u8; 32],
        scope: ActionScope,
        ops: Ops,
        redeem_amount_offset: u16,
        fee_num: u64,
        fee_den: u64,
    ) -> ProgramResult {
        validate_ops(&ops)?;

        let rent_exemption_lamports = Rent::get()?.minimum_balance(Action::SPACE);
        let create_account_ix = create_account(
            self.admin.key,
            self.action.key,
            rent_exemption_lamports,
            Action::SPACE as u64,
            &crate::ID,
        );
        let account_infos = [
            self.admin.clone(),
            self.action.clone(),
            self.system_program_acc.clone(),
        ];
        let bump = [self.action_bump];

        invoke_signed(
            &create_account_ix,
            &account_infos,
            &[&[Action::SEED, self.vault_key.as_ref(), &action_hash, &bump]],
        )?;

        let action = Action {
            vault: self.vault_key.to_bytes(),
            action_hash,
            ops,
            fee_num,
            fee_den,
            scope,
            redeem_amount_offset,
            bump: self.action_bump,
        };
        let serialized =
            serialize(&Account::Action(action)).map_err(|_| ProgramError::InvalidAccountData)?;
        if serialized.len() > Action::SPACE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut data = self.action.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}

/// Loads `[admin signer+writable, vault, action (writable)]`, verifies the vault
/// admin, and binds the action account to the PDA for `action_hash`.
pub(crate) struct RevokeActionContext<'a, 'info> {
    admin: &'a AccountInfo<'info>,
    action: &'a AccountInfo<'info>,
}

impl<'a, 'info> RevokeActionContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        action_hash: &[u8; 32],
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let admin = next_account(accounts_iter)?;
        require_writable_signer(admin)?;
        let vault_account = next_account(accounts_iter)?;
        let action = next_account(accounts_iter)?;
        require_writable(action)?;

        let context = VaultRoleContext::load(admin, vault_account, Role::Admin)?;
        let vault_key = context.vault_key();

        // The supplied account must be the Action PDA for the requested hash and
        // a real Action scoped to this vault.
        let (expected_action_key, _) = Action::find_address(&vault_key, action_hash);
        if action.key != &expected_action_key {
            return Err(ProgramError::InvalidSeeds);
        }
        let stored = Account::load_as::<Action>(action)?;
        stored.verify_vault(&vault_key)?;

        Ok(Self { admin, action })
    }

    /// Closes the Action account: drains its lamports to the admin, clears the
    /// data, and returns ownership to the system program.
    pub(crate) fn close(self) -> ProgramResult {
        close_account(self.action, self.admin)
    }
}
