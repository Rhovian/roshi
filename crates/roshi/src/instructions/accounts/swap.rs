use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;
use solana_sysvar::clock::Clock;

use super::{
    oracle_price::read_oracle_price,
    shared::{next_account, require_writable},
};
use crate::{
    instructions::{token, SwapArgs},
    oracle::{OracleConfig, OracleKind, OraclePrice},
    state::{
        action::{Action, ActionScope},
        asset::Asset,
        sub_account::VaultSubAccount,
        vault::{self, Role, Vault},
        Account,
    },
};
use roshi_interface::{error::RoshiError, math::base_atoms_from_asset_atoms};

/// Fixed swap account layout:
///
/// 0. `[signer]` Strategist (verified against `vault.strategist`).
/// 1. `[]` Vault.
/// 2. `[]` Subaccount PDA derived from `(vault, sub_account)`.
/// 3. `[writable]` Input custody token account (owner = subaccount PDA).
/// 4. `[writable]` Output custody token account (owner = subaccount PDA).
/// 5. `[]` Action PDA derived from `(vault, recomputed_action_hash)`.
/// 6. `..` Valuation accounts — only when `max_swap_slippage_bps > 0`: per
///    non-base endpoint (input side first), the registered Asset PDA followed
///    by its own oracle accounts; then, iff any endpoint is routed, the vault
///    base-oracle leg **once**, shared by both sides. Base endpoints consume
///    nothing.
/// 7. `..` CPI account section.
pub(crate) struct SwapContext<'a, 'info> {
    pub(crate) sub_account: &'a AccountInfo<'info>,
    pub(crate) input_custody: &'a AccountInfo<'info>,
    pub(crate) output_custody: &'a AccountInfo<'info>,
    pub(crate) cpi_accounts: &'a [AccountInfo<'info>],
    pub(crate) action: Action,
    pub(crate) vault: Vault,
    pub(crate) vault_key: Pubkey,
    pub(crate) sub_account_index: u8,
    pub(crate) sub_account_bump: u8,
    /// Endpoint pricing, present iff the vault's swap slippage bound is on.
    pub(crate) valuation: Option<SwapValuation<'a, 'info>>,
}

impl<'a, 'info> SwapContext<'a, 'info>
where
    'a: 'info,
{
    // The context carries a full `Vault` by value; keep its construction (and
    // the validation temporaries) on this function's own stack frame instead
    // of inlining them into the already-large swap handler.
    #[inline(never)]
    pub(crate) fn load(
        accounts: &'a [AccountInfo<'info>],
        args: &SwapArgs,
    ) -> Result<Self, ProgramError> {
        let accounts_iter = &mut accounts.iter();

        let strategist = next_account(accounts_iter)?;
        let vault_account = next_account(accounts_iter)?;
        let vault = vault::load_checked(vault_account)?;
        vault::verify_role(&vault, Role::Strategist, strategist)?;
        vault.verify_manage_enabled()?;

        let vault_key = *vault_account.key;
        let sub_account = next_account(accounts_iter)?;
        let sub_account_bump =
            VaultSubAccount::verify_account(&vault_key, args.sub_account, sub_account)?;

        let input_custody = next_account(accounts_iter)?;
        require_writable(input_custody)?;
        token::verify_custody_account(input_custody, sub_account.key)?;

        let output_custody = next_account(accounts_iter)?;
        require_writable(output_custody)?;
        token::verify_custody_account(output_custody, sub_account.key)?;

        if input_custody.key == output_custody.key {
            return Err(RoshiError::InvalidTokenAccount.into());
        }

        let action_account = next_account(accounts_iter)?;
        let action = Account::load_as::<Action>(action_account)?;
        action.verify_for_vault(&vault_key, action_account.key)?;
        if action.scope != ActionScope::Swap {
            return Err(RoshiError::UnauthorizedAction.into());
        }

        let mut remaining = accounts_iter.as_slice();
        let valuation = if vault.controls.max_swap_slippage_bps > 0 {
            let input_mint = token::token_account_mint(input_custody)?;
            let output_mint = token::token_account_mint(output_custody)?;
            let (valuation, used) =
                SwapValuation::parse(&vault, &vault_key, &input_mint, &output_mint, remaining)?;
            remaining = &remaining[used..];
            Some(valuation)
        } else {
            None
        };

        Ok(Self {
            sub_account,
            input_custody,
            output_custody,
            cpi_accounts: remaining,
            action,
            vault,
            vault_key,
            sub_account_index: args.sub_account,
            sub_account_bump,
            valuation,
        })
    }
}

/// Both endpoint valuations plus the vault base-oracle leg, which is parsed
/// **once** and shared: a single swap can never see two prices for the base
/// feed (a per-leg base account would let the caller value the two sides
/// against different verified updates of the same pull-oracle feed).
pub(crate) struct SwapValuation<'a, 'info> {
    input: LegPricing<'a, 'info>,
    output: LegPricing<'a, 'info>,
    /// The vault base-oracle accounts; present iff any endpoint is routed.
    base_leg: Option<&'a [AccountInfo<'info>]>,
}

impl<'a, 'info> SwapValuation<'a, 'info>
where
    'a: 'info,
{
    /// Parse the valuation section: the input endpoint's accounts, the output
    /// endpoint's, then the shared base leg when either endpoint routes.
    fn parse(
        vault: &Vault,
        vault_key: &Pubkey,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<(Self, usize), ProgramError> {
        let (input, used_input) = LegPricing::parse(vault, vault_key, input_mint, accounts)?;
        let (output, used_output) =
            LegPricing::parse(vault, vault_key, output_mint, &accounts[used_input..])?;
        let mut used = used_input
            .checked_add(used_output)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;

        let base_leg = if input.routed()? || output.routed()? {
            let count = leg_account_count(&vault.base_oracle)?;
            let leg = accounts
                .get(used..used + count)
                .ok_or(ProgramError::NotEnoughAccountKeys)?;
            used += count;
            Some(leg)
        } else {
            None
        };

        Ok((
            Self {
                input,
                output,
                base_leg,
            },
            used,
        ))
    }

    /// Value the realized `(spent, received)` amounts in base atoms, reading
    /// the shared base price once for both sides.
    pub(crate) fn values(
        &self,
        vault: &Vault,
        spent: u64,
        received: u64,
        clock: &Clock,
    ) -> Result<(u64, u64), ProgramError> {
        let base_price = match self.base_leg {
            Some(accounts) => Some(read_oracle_price(&vault.base_oracle, accounts, clock)?.0),
            None => None,
        };

        let spent_value = self
            .input
            .value_in_base_atoms(vault, spent, base_price, clock)?;
        let received_value = self
            .output
            .value_in_base_atoms(vault, received, base_price, clock)?;
        Ok((spent_value, received_value))
    }
}

/// How one swap endpoint is valued in base atoms.
enum LegPricing<'a, 'info> {
    /// The vault base mint: value = amount, no accounts.
    Base,
    /// A registered Asset: value through its oracle, exactly the pricing path
    /// deposits use. The asset's `enabled` flag gates deposits, not
    /// valuation, so a deposit-disabled asset still prices here.
    Asset {
        asset: Asset,
        oracle_accounts: &'a [AccountInfo<'info>],
    },
}

impl<'a, 'info> LegPricing<'a, 'info>
where
    'a: 'info,
{
    /// Parse one endpoint's valuation accounts from the front of `accounts`:
    /// nothing for the base mint, otherwise the registered Asset PDA plus its
    /// own oracle accounts (the routed base leg is shared, parsed by
    /// [`SwapValuation::parse`]). An endpoint that is neither the base mint
    /// nor a registered Asset is unpriceable and rejects the swap (settled
    /// posture: endpoints must price; aggregator multi-hop inside the CPI
    /// stays opaque).
    fn parse(
        vault: &Vault,
        vault_key: &Pubkey,
        mint: &Pubkey,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<(Self, usize), ProgramError> {
        if mint.to_bytes() == vault.base_mint {
            return Ok((Self::Base, 0));
        }

        let asset_account = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
        let (expected_asset, _) = Asset::find_address(vault_key, mint);
        if asset_account.key != &expected_asset {
            return Err(RoshiError::UnpriceableSwapLeg.into());
        }
        let asset = Account::load_as::<Asset>(asset_account)
            .map_err(|_| ProgramError::from(RoshiError::UnpriceableSwapLeg))?;

        let oracle_account_count = leg_account_count(&asset.oracle)?;
        let oracle_accounts = accounts
            .get(1..1 + oracle_account_count)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;

        Ok((
            Self::Asset {
                asset,
                oracle_accounts,
            },
            1 + oracle_account_count,
        ))
    }

    fn routed(&self) -> Result<bool, ProgramError> {
        match self {
            Self::Base => Ok(false),
            Self::Asset { asset, .. } => asset.routed(),
        }
    }

    /// Value `amount` of this endpoint's mint in base atoms. `base_price` is
    /// the swap's shared base-leg price, present whenever any endpoint is
    /// routed.
    fn value_in_base_atoms(
        &self,
        vault: &Vault,
        amount: u64,
        base_price: Option<OraclePrice>,
        clock: &Clock,
    ) -> Result<u64, ProgramError> {
        match self {
            Self::Base => Ok(amount),
            Self::Asset {
                asset,
                oracle_accounts,
            } => {
                let (asset_price, _) = read_oracle_price(&asset.oracle, oracle_accounts, clock)?;
                let base_price = if asset.routed()? {
                    // `SwapValuation::parse` provides the shared leg whenever
                    // an endpoint routes.
                    base_price.ok_or(ProgramError::InvalidAccountData)?
                } else {
                    OraclePrice::UNIT
                };

                base_atoms_from_asset_atoms(
                    amount,
                    asset_price,
                    base_price,
                    asset.asset_decimals,
                    vault.base_decimals,
                )
                .map_err(Into::into)
            }
        }
    }
}

/// Accounts one oracle leg consumes (Pyth: 1 price update; Switchboard:
/// quote, queue, slot-hashes sysvar, instructions sysvar).
fn leg_account_count(config: &OracleConfig) -> Result<usize, ProgramError> {
    // Holders of an OracleConfig validate the kind at deserialization, so an
    // invalid kind here is corrupted state.
    match config
        .kind()
        .map_err(|_| ProgramError::InvalidAccountData)?
    {
        OracleKind::Pyth => Ok(1),
        OracleKind::Switchboard => Ok(4),
    }
}
