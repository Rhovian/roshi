use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;
use solana_sysvar::clock::Clock;

use super::{
    oracle_price::{
        feeds_match, oracle_feed_identity, read_oracle_price, split_oracle_accounts,
        OracleFeedIdentity,
    },
    shared::{next_account, require_writable},
};
use crate::{
    instructions::{token, SwapArgs},
    oracle::OraclePrice,
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
/// 0. `[signer]` Swap executor (verified against `vault.swap_authority` or
///    `vault.strategist`).
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

        let swap_executor = next_account(accounts_iter)?;
        let vault_account = next_account(accounts_iter)?;
        let vault = vault::load_checked(vault_account)?;
        vault::verify_either_role(&vault, Role::SwapAuthority, Role::Strategist, swap_executor)?;
        vault.verify_manage_enabled()?;

        let vault_key = *vault_account.key;
        let sub_account = next_account(accounts_iter)?;
        let sub_account_bump =
            VaultSubAccount::verify_account(&vault_key, args.sub_account, sub_account)?;

        // Baseline: subaccount-owned, no close authority. An endpoint may carry a
        // pre-existing delegate (the flash-collateral repay delegate sits on the
        // output ATA), so a delegate is tolerated here — but `execution::swap` must
        // reverify post-CPI that the route changed nothing but the balance
        // (`verify_swap_endpoint_unchanged`), or the route could plant one.
        let input_custody = next_account(accounts_iter)?;
        require_writable(input_custody)?;
        token::verify_swap_endpoint_custody(input_custody, sub_account.key)?;

        let output_custody = next_account(accounts_iter)?;
        require_writable(output_custody)?;
        token::verify_swap_endpoint_custody(output_custody, sub_account.key)?;

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
            let (valuation, after_valuation) =
                SwapValuation::parse(&vault, &vault_key, &input_mint, &output_mint, remaining)?;
            remaining = after_valuation;
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
    ) -> Result<(Self, &'a [AccountInfo<'info>]), ProgramError> {
        let (input, remaining) = LegPricing::parse(vault, vault_key, input_mint, accounts)?;
        let (output, mut remaining) = LegPricing::parse(vault, vault_key, output_mint, remaining)?;

        let base_leg = if input.routed()? || output.routed()? {
            let (leg, after_leg) = split_oracle_accounts(&vault.base_oracle, remaining)?;
            remaining = after_leg;
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
            remaining,
        ))
    }

    /// Value the realized `(spent, received)` amounts in base atoms, reading
    /// each reuse-compatible oracle configuration exactly once.
    pub(crate) fn values(
        &self,
        vault: &Vault,
        spent: u64,
        received: u64,
        clock: &Clock,
    ) -> Result<(u64, u64), ProgramError> {
        // The base leg is read here; an endpoint may reuse it only when both
        // active oracle configurations are identical. The output endpoint may
        // likewise reuse the input price. This forbids valuing one feed against
        // two independently supplied updates without skipping a leg's own
        // verification policy. `value_in_base_atoms` still applies each leg's
        // routed/direct logic, so a routed leg that is the base prices at 1.0.
        let base_feed = match self.base_leg {
            Some(_) => Some(oracle_feed_identity(&vault.base_oracle)?),
            None => None,
        };
        let input_feed = self.input.feed_identity()?;
        let output_feed = self.output.feed_identity()?;
        let input_uses_base = feeds_match(input_feed, base_feed)?;
        let output_uses_base = feeds_match(output_feed, base_feed)?;
        let output_uses_input = feeds_match(output_feed, input_feed)?;
        let base_price = match self.base_leg {
            Some(accounts) => Some(read_oracle_price(&vault.base_oracle, accounts, clock)?.0),
            None => None,
        };

        let input_price = if input_uses_base {
            base_price
        } else {
            self.input.read_asset_price(clock)?
        };
        let output_price = if output_uses_base {
            base_price
        } else if output_uses_input {
            input_price
        } else {
            self.output.read_asset_price(clock)?
        };

        let spent_value = self
            .input
            .value_in_base_atoms(vault, spent, input_price, base_price)?;
        let received_value =
            self.output
                .value_in_base_atoms(vault, received, output_price, base_price)?;
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
    ) -> Result<(Self, &'a [AccountInfo<'info>]), ProgramError> {
        if mint.to_bytes() == vault.base_mint {
            return Ok((Self::Base, accounts));
        }

        let asset_account = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
        let (expected_asset, _) = Asset::find_address(vault_key, mint);
        if asset_account.key != &expected_asset {
            return Err(RoshiError::UnpriceableSwapLeg.into());
        }
        let asset = Account::load_as::<Asset>(asset_account)
            .map_err(|_| ProgramError::from(RoshiError::UnpriceableSwapLeg))?;

        let (oracle_accounts, remaining) = split_oracle_accounts(&asset.oracle, &accounts[1..])?;

        Ok((
            Self::Asset {
                asset,
                oracle_accounts,
            },
            remaining,
        ))
    }

    fn routed(&self) -> Result<bool, ProgramError> {
        match self {
            Self::Base => Ok(false),
            Self::Asset { asset, .. } => asset.routed(),
        }
    }

    /// This endpoint's oracle configuration identity, or `None` for the base
    /// mint. Reuse requires an exact semantic match; the same feed under a
    /// different verification policy is rejected.
    fn feed_identity(&self) -> Result<Option<OracleFeedIdentity>, ProgramError> {
        match self {
            Self::Base => Ok(None),
            Self::Asset { asset, .. } => Ok(Some(oracle_feed_identity(&asset.oracle)?)),
        }
    }

    /// Read this endpoint's verified asset price from its supplied oracle
    /// accounts, or `None` for the base mint (which prices one-to-one).
    fn read_asset_price(&self, clock: &Clock) -> Result<Option<OraclePrice>, ProgramError> {
        match self {
            Self::Base => Ok(None),
            Self::Asset {
                asset,
                oracle_accounts,
            } => {
                let (asset_price, _) = read_oracle_price(&asset.oracle, oracle_accounts, clock)?;
                Ok(Some(asset_price))
            }
        }
    }

    /// Value `amount` of this endpoint's mint in base atoms from a pre-read
    /// `asset_price` (`None` for the base mint). `base_price` is the swap's
    /// shared base-leg price, present whenever any endpoint is routed.
    fn value_in_base_atoms(
        &self,
        vault: &Vault,
        amount: u64,
        asset_price: Option<OraclePrice>,
        base_price: Option<OraclePrice>,
    ) -> Result<u64, ProgramError> {
        match self {
            Self::Base => Ok(amount),
            Self::Asset { asset, .. } => {
                let asset_price = asset_price.ok_or(ProgramError::InvalidAccountData)?;
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
