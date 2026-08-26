use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_sysvar::clock::Clock;

use crate::oracle::{
    ActiveOracleConfig, OracleConfig, OraclePrice, PythOracle, ScopeOracle, SwitchboardOracle,
};

const PYTH_LEG_ACCOUNTS: usize = 1; // PriceUpdateV2
const SWITCHBOARD_LEG_ACCOUNTS: usize = 4; // quote, queue, slot hashes, instructions
const SCOPE_LEG_ACCOUNTS: usize = 2; // OraclePrices, OracleMappings

/// Split one `N`-account leg from the front of `accounts`.
fn split_leg<'a, 'info, const N: usize>(
    accounts: &'a [AccountInfo<'info>],
) -> Result<(&'a [AccountInfo<'info>; N], &'a [AccountInfo<'info>]), ProgramError> {
    accounts
        .split_first_chunk::<N>()
        .ok_or(ProgramError::NotEnoughAccountKeys)
}

#[derive(Clone, Copy)]
pub(crate) struct OracleLeg<'a, 'info> {
    config: ActiveOracleConfig,
    accounts: &'a [AccountInfo<'info>],
}

impl<'a, 'info> OracleLeg<'a, 'info>
where
    'a: 'info,
{
    pub(crate) fn parse(
        oracle: &OracleConfig,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<(Self, &'a [AccountInfo<'info>]), ProgramError> {
        let config = oracle
            .active()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        let count = match config {
            ActiveOracleConfig::Pyth(_) => PYTH_LEG_ACCOUNTS,
            ActiveOracleConfig::Switchboard(_) => SWITCHBOARD_LEG_ACCOUNTS,
            ActiveOracleConfig::Scope(_) => SCOPE_LEG_ACCOUNTS,
        };
        let (accounts, remaining) = accounts
            .split_at_checked(count)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        Ok((Self { config, accounts }, remaining))
    }

    fn read(self, clock: &Clock) -> Result<OraclePrice, ProgramError> {
        match self.config {
            ActiveOracleConfig::Pyth(config) => {
                let ([price_account], _) = split_leg::<PYTH_LEG_ACCOUNTS>(self.accounts)?;
                PythOracle::new(config).read_verified_price(price_account, clock.unix_timestamp)
            }
            ActiveOracleConfig::Switchboard(config) => {
                let ([quote, queue, slothash, ix_sysvar], _) =
                    split_leg::<SWITCHBOARD_LEG_ACCOUNTS>(self.accounts)?;
                SwitchboardOracle::new(config)
                    .read_verified_price(quote, queue, slothash, ix_sysvar, clock.slot)
            }
            ActiveOracleConfig::Scope(config) => {
                let ([prices, mappings], _) = split_leg::<SCOPE_LEG_ACCOUNTS>(self.accounts)?;
                ScopeOracle::new(config).read_verified_price(prices, mappings, clock.unix_timestamp)
            }
        }
    }
}

/// Provider-specific identity of the underlying feed, independent of the
/// validation and interpretation policy applied to it. Scope's feed is the
/// pinned `OraclePrices` account plus the entry index: both legs read the same
/// on-chain account, so distinct entries are distinct feeds, while the source
/// mapping commitment is validation policy on the entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OracleFeedKey {
    Switchboard([u8; 32]),
    Pyth([u8; 32]),
    Scope {
        prices_account: [u8; 32],
        price_index: u16,
    },
}

const fn feed_key(config: ActiveOracleConfig) -> OracleFeedKey {
    match config {
        ActiveOracleConfig::Switchboard(config) => OracleFeedKey::Switchboard(config.feed_id),
        ActiveOracleConfig::Pyth(config) => OracleFeedKey::Pyth(config.feed_id),
        ActiveOracleConfig::Scope(config) => OracleFeedKey::Scope {
            prices_account: config.prices_account,
            price_index: config.price_index,
        },
    }
}

fn feeds_match(left: ActiveOracleConfig, right: ActiveOracleConfig) -> Result<bool, ProgramError> {
    if feed_key(left) != feed_key(right) {
        return Ok(false);
    }
    if left != right {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(true)
}

#[derive(Clone, Copy)]
struct VerifiedOracleLeg<'leg, 'a, 'info> {
    leg: &'leg OracleLeg<'a, 'info>,
    price: OraclePrice,
}

/// Reads at most the three legs in one valuation path. A repeated feed reuses
/// its first verified price only when both its complete config and account keys
/// match; a policy or account substitution fails before valuation.
pub(crate) struct OracleReadSession<'leg, 'a, 'info> {
    verified: [Option<VerifiedOracleLeg<'leg, 'a, 'info>>; 3],
    len: usize,
}

impl<'leg, 'a, 'info> OracleReadSession<'leg, 'a, 'info>
where
    'a: 'info,
{
    pub(crate) const fn new() -> Self {
        Self {
            verified: [None; 3],
            len: 0,
        }
    }

    pub(crate) fn read(
        &mut self,
        leg: &'leg OracleLeg<'a, 'info>,
        clock: &Clock,
    ) -> Result<OraclePrice, ProgramError> {
        let mut reused = None;
        for verified in self.verified[..self.len].iter().flatten() {
            if feeds_match(leg.config, verified.leg.config)? {
                if !same_account_keys(leg.accounts, verified.leg.accounts) {
                    return Err(ProgramError::InvalidAccountData);
                }
                reused = Some(verified.price);
            }
        }

        let price = match reused {
            Some(price) => price,
            None => leg.read(clock)?,
        };
        debug_assert!(self.len < self.verified.len());
        self.verified[self.len] = Some(VerifiedOracleLeg { leg, price });
        self.len += 1;
        Ok(price)
    }
}

fn same_account_keys(left: &[AccountInfo], right: &[AccountInfo]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.key == right.key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::{
        PythOracleConfig, ScopeOracleConfig, ScopeOracleMapping, SwitchboardOracleConfig,
    };

    fn active(config: &OracleConfig) -> ActiveOracleConfig {
        config.active().unwrap()
    }

    fn scope_mapping() -> ScopeOracleMapping {
        ScopeOracleMapping::new([3; 32], 26, 9, 3, 17, [4; 20])
    }

    fn scope_config(
        mapping: ScopeOracleMapping,
        price_index: u16,
        max_age_seconds: u64,
    ) -> OracleConfig {
        OracleConfig::scope(ScopeOracleConfig::new(
            [2; 32],
            mapping,
            price_index,
            max_age_seconds,
        ))
    }

    #[test]
    fn same_feed_rejects_mismatched_validation_policy() {
        let mapping = scope_mapping();
        let base = active(&scope_config(mapping, 445, 30));

        let mut different_mappings = Vec::new();
        let mut changed = mapping;
        changed.price_info_account = [8; 32];
        different_mappings.push(changed);
        changed = mapping;
        changed.price_type += 1;
        different_mappings.push(changed);
        changed = mapping;
        changed.twap_source_or_ref_price_tolerance_bps += 1;
        different_mappings.push(changed);
        changed = mapping;
        changed.twap_enabled_bitmask += 1;
        different_mappings.push(changed);
        changed = mapping;
        changed.ref_price += 1;
        different_mappings.push(changed);
        changed = mapping;
        changed.generic[0] ^= 1;
        different_mappings.push(changed);

        let mut different_policies = vec![scope_config(mapping, 445, 31)];
        different_policies.extend(
            different_mappings
                .into_iter()
                .map(|mapping| scope_config(mapping, 445, 30)),
        );
        for config in different_policies {
            assert_eq!(
                feeds_match(base, active(&config)),
                Err(ProgramError::InvalidAccountData)
            );
        }

        assert_eq!(
            feeds_match(base, active(&scope_config(mapping, 446, 30))),
            Ok(false)
        );
        assert_eq!(
            feeds_match(
                base,
                active(&OracleConfig::scope(ScopeOracleConfig::new(
                    [9; 32], mapping, 445, 30,
                )))
            ),
            Ok(false)
        );
        assert_eq!(feeds_match(base, base), Ok(true));
    }

    #[test]
    fn pyth_same_feed_rejects_mismatched_validation_policy() {
        let base = active(&OracleConfig::pyth(PythOracleConfig::new(
            [9; 32], 8, 30, 250,
        )));
        let different_policy = active(&OracleConfig::pyth(PythOracleConfig::new(
            [9; 32], 8, 31, 250,
        )));

        assert_eq!(
            feeds_match(base, different_policy),
            Err(ProgramError::InvalidAccountData)
        );
    }

    #[test]
    fn switchboard_same_feed_rejects_mismatched_validation_policy() {
        let base = active(&OracleConfig::switchboard(SwitchboardOracleConfig::new(
            [1; 32], [2; 32], [9; 32], 8, 30,
        )));
        let different_policy = active(&OracleConfig::switchboard(SwitchboardOracleConfig::new(
            [1; 32], [2; 32], [9; 32], 8, 31,
        )));

        assert_eq!(
            feeds_match(base, different_policy),
            Err(ProgramError::InvalidAccountData)
        );
    }
}
