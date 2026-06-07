use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_sysvar::{clock::Clock, Sysvar};
use wincode::serialize;

use super::shared::{next_account, require_writable};
use crate::{
    instructions::{
        token::{associated_token_address, verify_token_program_for},
        DepositArgs,
    },
    oracle::{OracleKind, OraclePrice, PythOracle, SwitchboardOracle},
    state::{asset::Asset, sub_account::VaultSubAccount, vault::Vault, Account},
};
use roshi_interface::{error::RoshiError, math::base_atoms_from_asset_atoms};

/// Fixed deposit account layout:
///
/// 0. `[signer]` Depositor.
/// 1. `[writable]` Vault.
/// 2. `[writable]` Depositor source token account.
/// 3. `[writable]` Vault custody token account.
/// 4. `[writable]` Depositor share token account.
/// 5. `[writable]` Share mint.
/// 6. `[]` Share SPL Token program.
/// 7. `[]` Asset SPL Token program.
/// 8. `[]` Asset PDA (non-base deposits only).
/// 9. `..` Oracle accounts (non-base deposits only; layout depends on the
///    asset's oracle kind).
pub(crate) struct DepositContext<'a, 'info> {
    pub(crate) depositor: &'a AccountInfo<'info>,
    pub(crate) vault_account: &'a AccountInfo<'info>,
    pub(crate) source: &'a AccountInfo<'info>,
    pub(crate) custody: &'a AccountInfo<'info>,
    pub(crate) share_dest: &'a AccountInfo<'info>,
    pub(crate) share_mint: &'a AccountInfo<'info>,
    pub(crate) share_token_program: &'a AccountInfo<'info>,
    pub(crate) asset_token_program: &'a AccountInfo<'info>,
    extra: &'a [AccountInfo<'info>],
    pub(crate) vault: Vault,
}

impl<'a, 'info> DepositContext<'a, 'info>
where
    'a: 'info,
{
    pub(crate) fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let depositor = next_account(accounts_iter)?;
        if !depositor.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
        let vault_account = next_account(accounts_iter)?;
        require_writable(vault_account)?;
        let source = next_account(accounts_iter)?;
        require_writable(source)?;
        let custody = next_account(accounts_iter)?;
        require_writable(custody)?;
        let share_dest = next_account(accounts_iter)?;
        require_writable(share_dest)?;
        let share_mint = next_account(accounts_iter)?;
        require_writable(share_mint)?;
        let share_token_program = next_account(accounts_iter)?;
        if share_token_program.key != &crate::instructions::token::TOKEN_PROGRAM_ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        let asset_token_program = next_account(accounts_iter)?;
        verify_token_program_for(asset_token_program, source)?;
        verify_token_program_for(asset_token_program, custody)?;
        let extra = accounts_iter.as_slice();

        let vault = Vault::load_checked(vault_account)?;
        vault.verify_share_mint(share_mint)?;

        Ok(Self {
            depositor,
            vault_account,
            source,
            custody,
            share_dest,
            share_mint,
            share_token_program,
            asset_token_program,
            extra,
            vault,
        })
    }

    /// Resolve the deposited amount into vault base atoms, verifying the custody
    /// account is the deposit sub-account's ATA for the mint. Base-mint deposits
    /// pass through 1:1; non-base deposits are priced through the asset's oracle.
    pub(crate) fn resolve_base_atoms(&self, args: &DepositArgs) -> Result<u64, ProgramError> {
        let vault_key = *self.vault_account.key;
        let base_mint = Pubkey::from(self.vault.base_mint);
        let asset_mint = Pubkey::from(args.asset_mint);

        // Custody is the deposit sub-account's ATA for the deposited mint, for
        // both base and non-base assets, so it is always vault-controlled.
        let (sub_account, _) =
            VaultSubAccount::find_address(&vault_key, self.vault.deposit_sub_account);
        if self.custody.key
            != &associated_token_address(&sub_account, &asset_mint, self.asset_token_program.key)
        {
            return Err(RoshiError::InvalidTokenAccount.into());
        }

        if asset_mint == base_mint {
            return Ok(args.amount);
        }

        let asset_account = self
            .extra
            .first()
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        let asset = Account::load_as::<Asset>(asset_account)?;
        let (expected_asset, _) = Asset::find_address(&vault_key, &asset_mint);
        if asset_account.key != &expected_asset
            || asset.vault != vault_key.to_bytes()
            || !asset.enabled()?
        {
            return Err(RoshiError::InvalidAssetAccount.into());
        }

        let price = self.read_oracle_price(&asset)?;
        base_atoms_from_asset_atoms(args.amount, price.value, price.decimals).map_err(Into::into)
    }

    fn read_oracle_price(&self, asset: &Asset) -> Result<OraclePrice, ProgramError> {
        let clock = Clock::get()?;

        match asset
            .oracle
            .kind()
            .map_err(|_| ProgramError::from(RoshiError::InvalidAssetAccount))?
        {
            OracleKind::Pyth => {
                let price_account = self
                    .extra
                    .get(1)
                    .ok_or(ProgramError::NotEnoughAccountKeys)?;
                PythOracle::new(asset.oracle.pyth)
                    .read_verified_price(price_account, clock.unix_timestamp)
            }
            OracleKind::Switchboard => {
                let quote = self
                    .extra
                    .get(1)
                    .ok_or(ProgramError::NotEnoughAccountKeys)?;
                let queue = self
                    .extra
                    .get(2)
                    .ok_or(ProgramError::NotEnoughAccountKeys)?;
                let slothash = self
                    .extra
                    .get(3)
                    .ok_or(ProgramError::NotEnoughAccountKeys)?;
                let ix_sysvar = self
                    .extra
                    .get(4)
                    .ok_or(ProgramError::NotEnoughAccountKeys)?;
                SwitchboardOracle::new(asset.oracle.switchboard)
                    .read_verified_price(quote, queue, slothash, ix_sysvar, clock.slot)
            }
        }
    }

    /// Apply `update` to the vault accounting and persist it.
    pub(crate) fn store(
        mut self,
        update: impl FnOnce(&mut Vault) -> ProgramResult,
    ) -> ProgramResult {
        update(&mut self.vault)?;

        let serialized =
            serialize(&Account::Vault(self.vault)).map_err(|_| ProgramError::InvalidAccountData)?;
        let mut data = self.vault_account.try_borrow_mut_data()?;
        if serialized.len() > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[..serialized.len()].copy_from_slice(&serialized);

        Ok(())
    }
}
