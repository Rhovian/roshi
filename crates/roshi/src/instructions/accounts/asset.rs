use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use super::{
    shared::{
        create_pda_account, next_account, require_system_program, require_uninitialized_account,
        require_writable, require_writable_signer,
    },
    vault::VaultRoleContext,
};
use crate::{
    instructions::{token, InitializeAssetArgs},
    state::{asset::Asset, vault::Role, Account},
};

/// Loads `[admin signer+writable, vault, asset_mint, asset (writable,
/// uninitialized), system program]`, verifies the vault admin, rejects the vault
/// base mint, validates the mint, and binds the asset account to the PDA for
/// `(vault, asset_mint)`.
pub(crate) struct InitializeAssetContext<'a, 'info> {
    admin: &'a AccountInfo<'info>,
    asset: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    vault_key: Pubkey,
    asset_mint: Pubkey,
    asset_bump: u8,
}

impl<'a, 'info> InitializeAssetContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        args: &InitializeAssetArgs,
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let admin = next_account(accounts_iter)?;
        require_writable_signer(admin)?;
        let vault_account = next_account(accounts_iter)?;
        let asset_mint_account = next_account(accounts_iter)?;
        let asset = next_account(accounts_iter)?;
        require_uninitialized_account(asset)?;
        let system_program_acc = next_account(accounts_iter)?;
        require_system_program(system_program_acc)?;

        let context = VaultRoleContext::load(admin, vault_account, Role::Admin)?;
        let vault_key = context.vault_key();
        let vault = context.vault();

        // The base mint is not a non-base deposit asset.
        if args.asset_mint == vault.base_mint {
            return Err(ProgramError::InvalidArgument);
        }

        let asset_mint = Pubkey::from(args.asset_mint);
        if asset_mint_account.key != &asset_mint {
            return Err(roshi_interface::error::RoshiError::InvalidMintAccount.into());
        }
        token::verify_mint(asset_mint_account, &asset_mint, args.asset_decimals, None)?;

        let (expected_asset_key, asset_bump) = Asset::find_address(&vault_key, &asset_mint);
        if asset.key != &expected_asset_key {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Self {
            admin,
            asset,
            system_program_acc,
            vault_key,
            asset_mint,
            asset_bump,
        })
    }

    pub(crate) fn vault_key(&self) -> Pubkey {
        self.vault_key
    }

    pub(crate) fn asset_bump(&self) -> u8 {
        self.asset_bump
    }

    /// Creates the rent-exempt Asset PDA (funded by the admin) and stores the
    /// already-validated config.
    pub(crate) fn create_and_store(&self, asset: Asset) -> ProgramResult {
        let bump = [self.asset_bump];
        create_pda_account(
            self.admin,
            self.asset,
            self.system_program_acc,
            Asset::SPACE,
            &crate::ID,
            &[
                Asset::SEED,
                self.vault_key.as_ref(),
                self.asset_mint.as_ref(),
                &bump,
            ],
        )?;

        let serialized =
            serialize(&Account::Asset(asset)).map_err(|_| ProgramError::InvalidAccountData)?;
        if serialized.len() > Asset::SPACE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut data = self.asset.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}

/// Loads `[admin signer, vault, asset (writable)]`, verifies the vault admin and
/// that the asset is the PDA for `(vault, asset.asset_mint)`, applies `update`
/// to the mutable fields, re-validates, and stores it.
pub(crate) fn update_writable_asset_as_admin(
    accounts: &[AccountInfo],
    update: impl FnOnce(&mut Asset) -> ProgramResult,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let admin = next_account(accounts_iter)?;
    let vault_account = next_account(accounts_iter)?;
    let asset_account = next_account(accounts_iter)?;
    require_writable(asset_account)?;

    let context = VaultRoleContext::load(admin, vault_account, Role::Admin)?;
    let vault_key = context.vault_key();

    let mut asset = Account::load_as::<Asset>(asset_account)?;
    let (expected_asset_key, expected_bump) =
        Asset::find_address(&vault_key, &Pubkey::from(asset.asset_mint));
    if asset_account.key != &expected_asset_key || asset.bump != expected_bump {
        return Err(ProgramError::InvalidSeeds);
    }

    update(&mut asset)?;
    asset.validate_state()?;

    let serialized =
        serialize(&Account::Asset(asset)).map_err(|_| ProgramError::InvalidAccountData)?;
    let mut data = asset_account.try_borrow_mut_data()?;
    if serialized.len() > data.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    data[..serialized.len()].copy_from_slice(&serialized);

    Ok(())
}
