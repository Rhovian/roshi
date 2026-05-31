use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_system_interface::instruction::create_account;
use solana_sysvar::{rent::Rent, Sysvar};
use wincode::serialize;

use crate::{
    instructions::{
        accounts::{
            next_account, require_system_program, require_uninitialized_account,
            require_writable_signer,
        },
        InitializeVaultArgs,
    },
    state::{program_config::ProgramConfig, vault::Vault, Account},
};

/// Implements [`crate::instructions::RoshiInstruction::InitializeVault`].
///
/// # Accounts
///
/// Account layout:
/// 0. `[signer]` Program authority stored in the program config account.
/// 1. `[]` Program config PDA derived from `ProgramConfig::SEED`.
/// 2. `[signer, writable]` Payer funding vault creation.
/// 3. `[writable]` Vault PDA derived from `[b"vault", tag, base_mint]`.
/// 4. `[]` System program.
///
/// # Implementation
///
/// Verifies the program authority gate, validates the vault tag and PDA seeds,
/// creates the vault account with rent-exempt lamports, records configured
/// role authorities, base-asset oracle config, and default subaccounts,
/// initializes fee, access, and NAV guardrail config, clears pause flags, and
/// starts accounting from an empty-share, empty-asset state.
pub fn try_initialize_vault(accounts: &[AccountInfo], args: InitializeVaultArgs) -> ProgramResult {
    let accounts = InitializeVaultAccounts::parse(accounts, &args)?;
    let tag = Vault::unpack_tag(&args.tag, args.tag_len)?;
    let vault = Vault::new(
        tag,
        args.admin,
        args.strategist,
        args.nav_authority,
        args.withdrawal_authority,
        args.base_mint,
        args.share_mint,
        args.base_decimals,
        args.base_oracle,
        args.deposit_sub_account,
        args.withdraw_sub_account,
        args.fee_collector,
        args.performance_fee_bps,
        args.withdrawal_buffer_bps,
        args.max_change_bps,
        args.min_update_interval,
        args.private,
        args.access_merkle_root,
        accounts.vault_bump,
    )?;
    let serialized =
        serialize(&Account::Vault(vault)).map_err(|_| ProgramError::InvalidAccountData)?;

    if serialized.len() > Vault::SPACE {
        return Err(ProgramError::InvalidAccountData);
    }

    accounts.create_vault_account()?;
    accounts.store_vault(&serialized)
}

struct InitializeVaultAccounts<'a, 'info> {
    payer: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    system_program_acc: &'a AccountInfo<'info>,
    tag: [u8; Vault::MAX_TAG_LEN],
    tag_len: u8,
    base_mint: Pubkey,
    vault_bump: u8,
}

impl<'a, 'info> InitializeVaultAccounts<'a, 'info> {
    fn parse(
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

        let system_program_acc = next_account(accounts_iter)?;
        require_system_program(system_program_acc)?;

        ProgramConfig::verify_authority(program_config, program_authority)?;

        let (tag, tag_len) = Vault::pack_tag(Vault::unpack_tag(&args.tag, args.tag_len)?)?;
        let base_mint = Pubkey::from(args.base_mint);
        let (expected_vault_key, vault_bump) =
            Vault::find_address(&tag[..usize::from(tag_len)], &base_mint)?;
        if vault.key != &expected_vault_key {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Self {
            payer,
            vault,
            system_program_acc,
            tag,
            tag_len,
            base_mint,
            vault_bump,
        })
    }

    fn create_vault_account(&self) -> ProgramResult {
        let rent_exemption_lamports = Rent::get()?.minimum_balance(Vault::SPACE);
        let create_account_ix = create_account(
            self.payer.key,
            self.vault.key,
            rent_exemption_lamports,
            Vault::SPACE as u64,
            &crate::ID,
        );
        let account_infos = [
            self.payer.clone(),
            self.vault.clone(),
            self.system_program_acc.clone(),
        ];
        let bump = [self.vault_bump];
        let tag = &self.tag[..usize::from(self.tag_len)];

        invoke_signed(
            &create_account_ix,
            &account_infos,
            &[&[Vault::SEED, tag, self.base_mint.as_ref(), &bump]],
        )
    }

    fn store_vault(&self, serialized: &[u8]) -> ProgramResult {
        let mut data = self.vault.try_borrow_mut_data()?;
        data[..serialized.len()].copy_from_slice(serialized);

        Ok(())
    }
}
