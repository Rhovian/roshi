use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_sysvar::clock::Clock;

use crate::oracle::{
    OracleConfig, OracleKind, OraclePrice, PythOracle, ScopeOracle, SwitchboardOracle,
};

/// Read one verified oracle leg from the front of `accounts`, returning its
/// price and the unconsumed accounts.
pub(crate) fn read_oracle_price<'a, 'info>(
    oracle: &OracleConfig,
    accounts: &'a [AccountInfo<'info>],
    clock: &Clock,
) -> Result<(OraclePrice, &'a [AccountInfo<'info>]), ProgramError>
where
    'a: 'info,
{
    // Both holders of an OracleConfig (vault, asset) validate the kind at
    // deserialization, so an invalid kind here is corrupted state.
    let kind = oracle
        .kind()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let (accounts, remaining) = split_oracle_accounts(oracle, accounts)?;

    let price = match kind {
        OracleKind::Pyth => {
            let price_account = &accounts[0];
            PythOracle::new(oracle.pyth_config())
                .read_verified_price(price_account, clock.unix_timestamp)?
        }
        OracleKind::Switchboard => {
            let quote = &accounts[0];
            let queue = &accounts[1];
            let slothash = &accounts[2];
            let ix_sysvar = &accounts[3];
            SwitchboardOracle::new(oracle.switchboard_config())
                .read_verified_price(quote, queue, slothash, ix_sysvar, clock.slot)?
        }
        OracleKind::Scope => {
            let prices = &accounts[0];
            let mappings = &accounts[1];
            ScopeOracle::new(oracle.scope_config()).read_verified_price(
                prices,
                mappings,
                clock.unix_timestamp,
            )?
        }
    };

    Ok((price, remaining))
}

/// Split one oracle leg from the front of `accounts`.
pub(crate) fn split_oracle_accounts<'a, 'info>(
    oracle: &OracleConfig,
    accounts: &'a [AccountInfo<'info>],
) -> Result<(&'a [AccountInfo<'info>], &'a [AccountInfo<'info>]), ProgramError> {
    let count = match oracle
        .kind()
        .map_err(|_| ProgramError::InvalidAccountData)?
    {
        OracleKind::Pyth => 1,        // PriceUpdateV2
        OracleKind::Switchboard => 4, // quote, queue, slot hashes, instructions
        OracleKind::Scope => 2,       // OraclePrices, OracleMappings
    };
    if accounts.len() < count {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    Ok(accounts.split_at(count))
}

/// The semantic active oracle configuration used to decide whether two pricing
/// legs may reuse one verified price. Explicit fields keep serialized padding
/// from becoming part of pricing behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OracleFeedIdentity {
    Switchboard {
        quote_account: [u8; 32],
        queue_account: [u8; 32],
        feed_id: [u8; 32],
        max_age_slots: u64,
        price_decimals: u8,
    },
    Pyth {
        feed_id: [u8; 32],
        price_update_account: [u8; 32],
        max_age_seconds: u64,
        max_confidence_bps: u16,
        price_decimals: u8,
    },
    Scope {
        prices_account: [u8; 32],
        price_info_account: [u8; 32],
        max_age_seconds: u64,
        price_index: u16,
        price_type: u8,
    },
}

/// Provider-specific identity of the underlying feed, independent of the
/// validation and interpretation policy applied to it. Scope's feed is the
/// pinned `OraclePrices` account plus the entry index: both legs read the same
/// on-chain account, so distinct entries are distinct feeds, while the source
/// pin (`price_info_account`, `price_type`) is validation policy on the entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OracleFeedKey {
    Switchboard([u8; 32]),
    Pyth([u8; 32]),
    Scope {
        prices_account: [u8; 32],
        price_index: u16,
    },
}

impl OracleFeedIdentity {
    const fn key(self) -> OracleFeedKey {
        match self {
            Self::Switchboard { feed_id, .. } => OracleFeedKey::Switchboard(feed_id),
            Self::Pyth { feed_id, .. } => OracleFeedKey::Pyth(feed_id),
            Self::Scope {
                prices_account,
                price_index,
                ..
            } => OracleFeedKey::Scope {
                prices_account,
                price_index,
            },
        }
    }
}

pub(crate) fn oracle_feed_identity(
    config: &OracleConfig,
) -> Result<OracleFeedIdentity, ProgramError> {
    match config
        .kind()
        .map_err(|_| ProgramError::InvalidAccountData)?
    {
        OracleKind::Switchboard => {
            let config = config.switchboard_config();
            Ok(OracleFeedIdentity::Switchboard {
                quote_account: config.quote_account,
                queue_account: config.queue_account,
                feed_id: config.feed_id,
                max_age_slots: config.max_age_slots,
                price_decimals: config.price_decimals,
            })
        }
        OracleKind::Pyth => {
            let config = config.pyth_config();
            Ok(OracleFeedIdentity::Pyth {
                feed_id: config.feed_id,
                price_update_account: config.price_update_account,
                max_age_seconds: config.max_age_seconds,
                max_confidence_bps: config.max_confidence_bps,
                price_decimals: config.price_decimals,
            })
        }
        OracleKind::Scope => {
            let config = config.scope_config();
            Ok(OracleFeedIdentity::Scope {
                prices_account: config.prices_account,
                price_info_account: config.price_info_account,
                max_age_seconds: config.max_age_seconds,
                price_index: config.price_index,
                price_type: config.price_type,
            })
        }
    }
}

/// Reuse is safe only when both legs name the same feed and apply the same
/// policy. Treating a policy mismatch as two feeds would let a caller compare
/// two independently supplied updates for one feed.
pub(crate) fn feeds_match(
    left: Option<OracleFeedIdentity>,
    right: Option<OracleFeedIdentity>,
) -> Result<bool, ProgramError> {
    let (Some(left), Some(right)) = (left, right) else {
        return Ok(false);
    };
    if left.key() != right.key() {
        return Ok(false);
    }
    if left != right {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::{PythOracleConfig, ScopeOracleConfig, SwitchboardOracleConfig};

    fn scope_config(
        price_info_account: [u8; 32],
        price_type: u8,
        price_index: u16,
        max_age_seconds: u64,
    ) -> OracleConfig {
        OracleConfig::scope(ScopeOracleConfig::new(
            [2; 32],
            price_info_account,
            price_type,
            price_index,
            max_age_seconds,
        ))
    }

    #[test]
    fn same_feed_rejects_mismatched_validation_policy() {
        let base = oracle_feed_identity(&scope_config([3; 32], 26, 445, 30)).unwrap();

        // Same entry under a different max age or source pin is one feed with
        // two policies, not two feeds.
        for config in [
            scope_config([3; 32], 26, 445, 31),
            scope_config([8; 32], 26, 445, 30),
            scope_config([3; 32], 27, 445, 30),
        ] {
            let different_policy = oracle_feed_identity(&config).unwrap();
            assert_eq!(
                feeds_match(Some(base), Some(different_policy)),
                Err(ProgramError::InvalidAccountData)
            );
        }

        // A different entry, or a different prices account, is a different feed.
        let different_entry = oracle_feed_identity(&scope_config([3; 32], 26, 446, 30)).unwrap();
        assert_eq!(feeds_match(Some(base), Some(different_entry)), Ok(false));
        let different_account = oracle_feed_identity(&OracleConfig::scope(ScopeOracleConfig::new(
            [9; 32], [3; 32], 26, 445, 30,
        )))
        .unwrap();
        assert_eq!(feeds_match(Some(base), Some(different_account)), Ok(false));

        assert_eq!(feeds_match(Some(base), Some(base)), Ok(true));
    }

    #[test]
    fn pyth_same_feed_rejects_mismatched_validation_policy() {
        let base = oracle_feed_identity(&OracleConfig::pyth(PythOracleConfig::new(
            [9; 32], 8, 30, 250,
        )))
        .unwrap();
        let different_policy = oracle_feed_identity(&OracleConfig::pyth(PythOracleConfig::new(
            [9; 32], 8, 31, 250,
        )))
        .unwrap();

        assert_eq!(
            feeds_match(Some(base), Some(different_policy)),
            Err(ProgramError::InvalidAccountData)
        );
    }

    #[test]
    fn switchboard_same_feed_rejects_mismatched_validation_policy() {
        let base = oracle_feed_identity(&OracleConfig::switchboard(SwitchboardOracleConfig::new(
            [1; 32], [2; 32], [9; 32], 8, 30,
        )))
        .unwrap();
        let different_policy = oracle_feed_identity(&OracleConfig::switchboard(
            SwitchboardOracleConfig::new([1; 32], [2; 32], [9; 32], 8, 31),
        ))
        .unwrap();

        assert_eq!(
            feeds_match(Some(base), Some(different_policy)),
            Err(ProgramError::InvalidAccountData)
        );
    }
}
