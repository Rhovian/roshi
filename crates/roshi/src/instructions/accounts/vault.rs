use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::serialize;

use super::{
    program_config::ProgramConfigAuthorityContext,
    shared::{
        create_pda_account, next_account, require_system_program, require_uninitialized_account,
        require_writable, require_writable_signer,
    },
};
use crate::{
    instructions::{token, InitializeVaultArgs},
    state::{
        vault::{self, Role, Vault},
        Account,
    },
};
use roshi_interface::{find_share_mint_address, math::SHARE_DECIMALS, SHARE_MINT_SEED};

pub(crate) struct VaultRoleContext<'a, 'info> {
    vault_account: &'a AccountInfo<'info>,
    vault: Vault,
}

impl<'a, 'info> VaultRoleContext<'a, 'info> {
    pub(crate) fn load(
        authority: &AccountInfo,
        vault_account: &'a AccountInfo<'info>,
        role: Role,
    ) -> Result<Self, ProgramError> {
        let vault = vault::load_checked(vault_account)?;
        vault::verify_role(&vault, role, authority)?;

        Ok(Self {
            vault_account,
            vault,
        })
    }

    pub(crate) fn vault(&self) -> &Vault {
        &self.vault
    }

    pub(crate) fn vault_key(&self) -> Pubkey {
        *self.vault_account.key
    }
}

pub(crate) struct WritableVaultRoleContext<'a, 'info> {
    vault_account: &'a AccountInfo<'info>,
    vault: Vault,
}

impl<'a, 'info> WritableVaultRoleContext<'a, 'info> {
    pub(crate) fn load(
        authority: &AccountInfo,
        vault_account: &'a AccountInfo<'info>,
        role: Role,
    ) -> Result<Self, ProgramError> {
        require_writable(vault_account)?;

        let context = VaultRoleContext::load(authority, vault_account, role)?;

        Ok(Self {
            vault_account: context.vault_account,
            vault: context.vault,
        })
    }

    pub(crate) fn vault_mut(&mut self) -> &mut Vault {
        &mut self.vault
    }

    pub(crate) fn vault(&self) -> &Vault {
        &self.vault
    }

    pub(crate) fn store(self) -> ProgramResult {
        self.vault.validate_state()?;

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

pub(crate) fn update_writable_vault_as_admin(
    accounts: &[AccountInfo],
    update: impl FnOnce(&mut Vault) -> ProgramResult,
) -> ProgramResult {
    let mut accounts_iter = accounts.iter();
    let admin = next_account(&mut accounts_iter)?;
    let vault_account = next_account(&mut accounts_iter)?;

    let mut context = WritableVaultRoleContext::load(admin, vault_account, Role::Admin)?;
    update(context.vault_mut())?;
    context.store()
}

pub(crate) struct InitializeVaultContext<'a, 'info> {
    payer: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    base_mint_account: &'a AccountInfo<'info>,
    share_mint_account: &'a AccountInfo<'info>,
    treasury: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    token_program_acc: &'a AccountInfo<'info>,
    tag: [u8; Vault::MAX_TAG_LEN],
    tag_len: u8,
    base_mint: Pubkey,
    share_mint_bump: u8,
    vault_bump: u8,
}

impl<'a, 'info> InitializeVaultContext<'a, 'info> {
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        args: &InitializeVaultArgs,
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let program_authority = next_account(accounts_iter)?;
        let program_config = next_account(accounts_iter)?;
        let payer = next_account(accounts_iter)?;
        require_writable_signer(payer)?;

        let vault = next_account(accounts_iter)?;
        require_uninitialized_account(vault)?;

        let base_mint_account = next_account(accounts_iter)?;
        let share_mint_account = next_account(accounts_iter)?;
        require_writable(share_mint_account)?;
        require_uninitialized_account(share_mint_account)?;
        let treasury = next_account(accounts_iter)?;

        let system_program_acc = next_account(accounts_iter)?;
        require_system_program(system_program_acc)?;
        let token_program_acc = next_account(accounts_iter)?;
        if token_program_acc.key != &token::TOKEN_PROGRAM_ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        ProgramConfigAuthorityContext::load(program_authority, program_config)?;

        let (tag, tag_len) = Vault::pack_tag(Vault::unpack_tag(&args.tag, args.tag_len)?)?;
        let base_mint = Pubkey::from(args.base_mint);
        let (expected_vault_key, vault_bump) =
            Vault::find_address(&tag[..usize::from(tag_len)], &base_mint)?;
        if vault.key != &expected_vault_key {
            return Err(ProgramError::InvalidSeeds);
        }
        let (expected_share_mint_key, share_mint_bump) = find_share_mint_address(vault.key);
        if share_mint_account.key != &expected_share_mint_key {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Self {
            payer,
            vault,
            base_mint_account,
            share_mint_account,
            treasury,
            system_program_acc,
            token_program_acc,
            tag,
            tag_len,
            base_mint,
            share_mint_bump,
            vault_bump,
        })
    }

    pub(crate) fn vault_bump(&self) -> u8 {
        self.vault_bump
    }

    pub(crate) fn share_mint(&self) -> [u8; 32] {
        self.share_mint_account.key.to_bytes()
    }

    /// Validate immutable external token accounts before the vault is stored.
    pub(crate) fn verify_external_token_accounts(
        &self,
        args: &InitializeVaultArgs,
    ) -> ProgramResult {
        token::verify_mint(
            self.base_mint_account,
            &self.base_mint,
            args.base_decimals,
            None,
        )?;
        token::verify_token_account_mint(self.treasury, &self.base_mint)?;
        if self.treasury.key != &Pubkey::from(args.treasury) {
            return Err(roshi_interface::error::RoshiError::InvalidTokenAccount.into());
        }

        Ok(())
    }

    pub(crate) fn create_share_mint(&self) -> ProgramResult {
        let bump = [self.share_mint_bump];
        create_pda_account(
            self.payer,
            self.share_mint_account,
            self.system_program_acc,
            token::MINT_LEN,
            &token::TOKEN_PROGRAM_ID,
            &[SHARE_MINT_SEED, self.vault.key.as_ref(), &bump],
        )?;

        token::initialize_mint(
            self.token_program_acc,
            self.share_mint_account,
            self.vault.key,
            SHARE_DECIMALS,
        )
    }

    pub(crate) fn create_vault_account(&self) -> ProgramResult {
        let bump = [self.vault_bump];
        let tag = &self.tag[..usize::from(self.tag_len)];
        create_pda_account(
            self.payer,
            self.vault,
            self.system_program_acc,
            Vault::SPACE,
            &crate::ID,
            &[Vault::SEED, tag, self.base_mint.as_ref(), &bump],
        )
    }

    pub(crate) fn store_vault(&self, serialized: &[u8]) -> ProgramResult {
        let mut data = self.vault.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(serialized);

        Ok(())
    }
}
