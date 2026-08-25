use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;

use super::{Oracle, OraclePrice, ScopeOracleConfig};

// Kamino Scope account layout (Kamino-Finance/scope, Anchor zero-copy).
//
// `OraclePrices { oracle_mappings: Pubkey, prices: [DatedPrice; 512] }` with
// `DatedPrice { price: Price { value: u64, exp: u64 }, last_updated_slot: u64,
// unix_timestamp: u64, generic_data: [u8; 24] }` (56 bytes per entry).
//
// `OracleMappings` is a struct of arrays; the two Roshi reads are
// `price_info_accounts: [Pubkey; 512]` (for Chainlink-sourced entries the
// 32-byte Data Streams feed id, stored verbatim) and `price_types: [u8; 512]`
// with the frozen flag in bit 7.
const ORACLE_PRICES_DISCRIMINATOR: &[u8; 8] = &[89, 128, 118, 221, 6, 72, 180, 146];
const ORACLE_PRICES_LEN: usize = 28_712;
const ORACLE_PRICES_MAPPINGS_OFFSET: usize = 8;
const DATED_PRICES_OFFSET: usize = 40;
const DATED_PRICE_SIZE: usize = 56;

const ORACLE_MAPPINGS_DISCRIMINATOR: &[u8; 8] = &[40, 244, 110, 80, 255, 214, 243, 188];
const ORACLE_MAPPINGS_LEN: usize = 29_704;
const PRICE_INFO_ACCOUNTS_OFFSET: usize = 8;
const PRICE_TYPES_OFFSET: usize = 16_392;

/// Scope `OracleType::ChainlinkExchangeRate` (Chainlink Data Streams report
/// schema V7, `exchangeRate`) — the only source type Roshi accepts, so the
/// entry is guaranteed to hold an on-chain-verified Chainlink report value.
const ORACLE_TYPE_CHAINLINK_EXCHANGE_RATE: u8 = 38;
/// Bit 7 of `price_types[i]` marks the entry frozen by the Scope admin.
const FROZEN_FLAG: u8 = 0x80;

/// Scope exponents never exceed 18 (`decimal_to_price` caps them); a larger
/// value is corrupted or foreign data.
const MAX_EXP: u64 = 18;

/// Kamino Scope cached-price reader.
///
/// Scope's Chainlink refresh path verifies Data Streams DON signatures
/// on-chain (CPI into the pinned Chainlink verifier) before caching the value,
/// and stamps `unix_timestamp` with the report's observation time clamped to
/// the cluster clock, enforced strictly monotonic across refreshes. Roshi
/// therefore only reads: it pins the prices account, its owner, the mappings
/// account the prices account itself declares, and the entry's source binding
/// (type + Chainlink feed id) on every read.
pub struct ScopeOracle {
    pub config: ScopeOracleConfig,
}

impl ScopeOracle {
    pub const fn new(config: ScopeOracleConfig) -> Self {
        Self { config }
    }

    /// Parse the configured entry from raw `OraclePrices` account bytes
    /// without account pinning, mapping, or freshness checks. This is useful
    /// for tests and off-chain inspection.
    pub fn parse_unverified_price(&self, data: &[u8]) -> Option<OraclePrice> {
        if data.len() != ORACLE_PRICES_LEN || &data[..8] != ORACLE_PRICES_DISCRIMINATOR {
            return None;
        }

        let entry = DatedPrice::parse(data, self.config.price_index)?;
        price_from_entry(&entry)
    }

    /// Read the configured entry as a fully verified price.
    ///
    /// Checks, in order: the prices account address and owner pin, both
    /// account discriminators and lengths, that `mappings_account` is the one
    /// the prices account declares, that the mapping entry is an unfrozen
    /// `ChainlinkExchangeRate` still bound to the configured feed id, and that
    /// the cached observation is positive, sanely scaled, not from the future,
    /// and within `max_age_seconds` of `unix_timestamp` (the current cluster
    /// time).
    pub fn read_verified_price(
        &self,
        prices_account: &AccountInfo,
        mappings_account: &AccountInfo,
        unix_timestamp: i64,
    ) -> Result<OraclePrice, ProgramError> {
        if prices_account.key.to_bytes() != self.config.prices_account {
            return Err(ProgramError::InvalidAccountData);
        }
        if prices_account.owner.to_bytes() != self.config.scope_program
            || mappings_account.owner.to_bytes() != self.config.scope_program
        {
            return Err(ProgramError::IllegalOwner);
        }

        let prices_data = prices_account.data.borrow();
        if prices_data.len() != ORACLE_PRICES_LEN
            || &prices_data[..8] != ORACLE_PRICES_DISCRIMINATOR
        {
            return Err(ProgramError::InvalidAccountData);
        }

        // The prices account itself declares its mappings account; the passed
        // account must be exactly that one.
        let declared_mappings =
            &prices_data[ORACLE_PRICES_MAPPINGS_OFFSET..ORACLE_PRICES_MAPPINGS_OFFSET + 32];
        if declared_mappings != mappings_account.key.to_bytes() {
            return Err(ProgramError::InvalidAccountData);
        }

        let mappings_data = mappings_account.data.borrow();
        self.verify_mapping_entry(&mappings_data)?;

        let entry = DatedPrice::parse(&prices_data, self.config.price_index)
            .ok_or(ProgramError::InvalidAccountData)?;
        self.verify_freshness(&entry, unix_timestamp)?;

        price_from_entry(&entry).ok_or(ProgramError::InvalidAccountData)
    }

    /// Require the mapping entry at the configured index to be an unfrozen
    /// `ChainlinkExchangeRate` still bound to the configured Chainlink feed
    /// id. A Scope admin rebinding of the index changes one of these and the
    /// read fails loudly (a rebind also resets the price entry on Scope's
    /// side).
    fn verify_mapping_entry(&self, mappings_data: &[u8]) -> Result<(), ProgramError> {
        if mappings_data.len() != ORACLE_MAPPINGS_LEN
            || &mappings_data[..8] != ORACLE_MAPPINGS_DISCRIMINATOR
        {
            return Err(ProgramError::InvalidAccountData);
        }

        let index = usize::from(self.config.price_index);
        if self.config.price_index >= ScopeOracleConfig::MAX_ENTRIES {
            return Err(ProgramError::InvalidAccountData);
        }

        let price_type = mappings_data[PRICE_TYPES_OFFSET + index];
        if price_type & FROZEN_FLAG != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        if price_type & !FROZEN_FLAG != ORACLE_TYPE_CHAINLINK_EXCHANGE_RATE {
            return Err(ProgramError::InvalidAccountData);
        }

        let feed_offset = PRICE_INFO_ACCOUNTS_OFFSET + 32 * index;
        if mappings_data[feed_offset..feed_offset + 32] != self.config.feed_id {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(())
    }

    /// Freshness is judged on the entry's `unix_timestamp` — the Chainlink
    /// observation time (clamped to the cluster clock at refresh), never the
    /// refresh or read time.
    fn verify_freshness(
        &self,
        entry: &DatedPrice,
        unix_timestamp: i64,
    ) -> Result<(), ProgramError> {
        let now = u64::try_from(unix_timestamp).map_err(|_| ProgramError::InvalidAccountData)?;
        if entry.unix_timestamp > now {
            return Err(ProgramError::InvalidAccountData);
        }
        if now - entry.unix_timestamp > self.config.max_age_seconds {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }
}

impl Oracle for ScopeOracle {
    fn parse_price(&self, data: &[u8]) -> Option<OraclePrice> {
        self.parse_unverified_price(data)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DatedPrice {
    value: u64,
    exp: u64,
    unix_timestamp: u64,
}

impl DatedPrice {
    /// Read the entry at `price_index` from `OraclePrices` account bytes the
    /// caller has already length-checked.
    fn parse(data: &[u8], price_index: u16) -> Option<Self> {
        if price_index >= ScopeOracleConfig::MAX_ENTRIES {
            return None;
        }

        let base = DATED_PRICES_OFFSET + DATED_PRICE_SIZE * usize::from(price_index);
        let field = |offset: usize| {
            data.get(base + offset..base + offset + 8)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u64::from_le_bytes)
        };

        Some(Self {
            value: field(0)?,
            exp: field(8)?,
            unix_timestamp: field(24)?,
        })
    }
}

fn price_from_entry(entry: &DatedPrice) -> Option<OraclePrice> {
    if entry.value == 0 || entry.exp > MAX_EXP {
        return None;
    }

    Some(OraclePrice {
        value: u128::from(entry.value),
        // Scope's exponent is value-dependent; it is taken from the entry on
        // every read, never assumed constant.
        decimals: entry.exp as u8,
    })
}

#[cfg(test)]
mod tests {
    use solana_pubkey::Pubkey;

    use super::*;

    const SCOPE_PROGRAM: Pubkey =
        solana_pubkey::pubkey!("HFn8GnPADiny6XqUoWE8uRPPxb29ikn4yTuPa9MF2fWJ");
    const PRICES_KEY: Pubkey =
        solana_pubkey::pubkey!("3t4JZcueEzTbVP6kLxXrL3VpWx45jDer4eqysweBchNH");
    const MAPPINGS_KEY: Pubkey =
        solana_pubkey::pubkey!("4zh6bmb77qX2CL7t5AJYCqa6YqFafbz3QJNeFvZjLowg");

    /// Live REUSD/USD (exchange rate) mainnet entry, dumped 2026-08-24.
    const REUSD_FEED_ID: [u8; 32] = [
        0x00, 0x07, 0x2c, 0x74, 0xe4, 0xa2, 0x8e, 0x4c, 0xb9, 0x3c, 0xf0, 0x55, 0x1e, 0x1b, 0xd9,
        0x92, 0xaf, 0x74, 0x3d, 0xb3, 0x7c, 0xe7, 0x91, 0xd6, 0x5c, 0xcc, 0xad, 0xd7, 0x55, 0x6f,
        0x5c, 0x40,
    ];
    const REUSD_INDEX: u16 = 445;
    const REUSD_VALUE: u64 = 109_679_858_068_090_600;
    const REUSD_EXP: u64 = 17;
    const REUSD_TIMESTAMP: u64 = 1_787_619_070;

    fn config() -> ScopeOracleConfig {
        ScopeOracleConfig::new(
            SCOPE_PROGRAM.to_bytes(),
            PRICES_KEY.to_bytes(),
            REUSD_FEED_ID,
            REUSD_INDEX,
            300,
        )
    }

    fn prices_data(mappings: &Pubkey, index: u16, value: u64, exp: u64, timestamp: u64) -> Vec<u8> {
        let mut data = vec![0u8; ORACLE_PRICES_LEN];
        data[..8].copy_from_slice(ORACLE_PRICES_DISCRIMINATOR);
        data[ORACLE_PRICES_MAPPINGS_OFFSET..ORACLE_PRICES_MAPPINGS_OFFSET + 32]
            .copy_from_slice(&mappings.to_bytes());
        let base = DATED_PRICES_OFFSET + DATED_PRICE_SIZE * usize::from(index);
        data[base..base + 8].copy_from_slice(&value.to_le_bytes());
        data[base + 8..base + 16].copy_from_slice(&exp.to_le_bytes());
        data[base + 24..base + 32].copy_from_slice(&timestamp.to_le_bytes());
        data
    }

    fn mappings_data(index: u16, price_type: u8, feed_id: [u8; 32]) -> Vec<u8> {
        let mut data = vec![0u8; ORACLE_MAPPINGS_LEN];
        data[..8].copy_from_slice(ORACLE_MAPPINGS_DISCRIMINATOR);
        data[PRICE_TYPES_OFFSET + usize::from(index)] = price_type;
        let feed_offset = PRICE_INFO_ACCOUNTS_OFFSET + 32 * usize::from(index);
        data[feed_offset..feed_offset + 32].copy_from_slice(&feed_id);
        data
    }

    fn reusd_prices_data() -> Vec<u8> {
        prices_data(
            &MAPPINGS_KEY,
            REUSD_INDEX,
            REUSD_VALUE,
            REUSD_EXP,
            REUSD_TIMESTAMP,
        )
    }

    fn reusd_mappings_data() -> Vec<u8> {
        mappings_data(
            REUSD_INDEX,
            ORACLE_TYPE_CHAINLINK_EXCHANGE_RATE,
            REUSD_FEED_ID,
        )
    }

    fn read(
        config: ScopeOracleConfig,
        prices_key: &Pubkey,
        prices_owner: &Pubkey,
        prices_data: &mut [u8],
        mappings_key: &Pubkey,
        mappings_owner: &Pubkey,
        mappings_data: &mut [u8],
        unix_timestamp: i64,
    ) -> Result<OraclePrice, ProgramError> {
        let mut prices_lamports = 1;
        let mut mappings_lamports = 1;
        let prices_account = AccountInfo::new(
            prices_key,
            false,
            false,
            &mut prices_lamports,
            prices_data,
            prices_owner,
            false,
        );
        let mappings_account = AccountInfo::new(
            mappings_key,
            false,
            false,
            &mut mappings_lamports,
            mappings_data,
            mappings_owner,
            false,
        );
        ScopeOracle::new(config).read_verified_price(
            &prices_account,
            &mappings_account,
            unix_timestamp,
        )
    }

    fn read_reusd(
        prices_data: &mut [u8],
        mappings_data: &mut [u8],
        unix_timestamp: i64,
    ) -> Result<OraclePrice, ProgramError> {
        read(
            config(),
            &PRICES_KEY,
            &SCOPE_PROGRAM,
            prices_data,
            &MAPPINGS_KEY,
            &SCOPE_PROGRAM,
            mappings_data,
            unix_timestamp,
        )
    }

    #[test]
    fn decodes_live_reusd_entry_fixture() {
        let price = read_reusd(
            &mut reusd_prices_data(),
            &mut reusd_mappings_data(),
            REUSD_TIMESTAMP as i64 + 60,
        )
        .unwrap();

        // 1.096798580680906 REUSD/USD, exactly as observed on mainnet.
        assert_eq!(
            price,
            OraclePrice {
                value: 109_679_858_068_090_600,
                decimals: 17,
            }
        );
    }

    #[test]
    fn enforces_freshness_on_observation_time() {
        let now = REUSD_TIMESTAMP as i64;

        // At the configured max age exactly: fresh.
        assert!(read_reusd(
            &mut reusd_prices_data(),
            &mut reusd_mappings_data(),
            now + 300
        )
        .is_ok());
        // One second past: stale.
        assert!(read_reusd(
            &mut reusd_prices_data(),
            &mut reusd_mappings_data(),
            now + 301
        )
        .is_err());
        // Observation from the future: rejected.
        assert!(read_reusd(
            &mut reusd_prices_data(),
            &mut reusd_mappings_data(),
            now - 1
        )
        .is_err());
    }

    #[test]
    fn rejects_wrong_prices_account_or_owner() {
        let other = Pubkey::new_unique();

        assert_eq!(
            read(
                config(),
                &other,
                &SCOPE_PROGRAM,
                &mut reusd_prices_data(),
                &MAPPINGS_KEY,
                &SCOPE_PROGRAM,
                &mut reusd_mappings_data(),
                REUSD_TIMESTAMP as i64,
            ),
            Err(ProgramError::InvalidAccountData)
        );
        assert_eq!(
            read(
                config(),
                &PRICES_KEY,
                &other,
                &mut reusd_prices_data(),
                &MAPPINGS_KEY,
                &SCOPE_PROGRAM,
                &mut reusd_mappings_data(),
                REUSD_TIMESTAMP as i64,
            ),
            Err(ProgramError::IllegalOwner)
        );
        assert_eq!(
            read(
                config(),
                &PRICES_KEY,
                &SCOPE_PROGRAM,
                &mut reusd_prices_data(),
                &MAPPINGS_KEY,
                &other,
                &mut reusd_mappings_data(),
                REUSD_TIMESTAMP as i64,
            ),
            Err(ProgramError::IllegalOwner)
        );
    }

    #[test]
    fn rejects_mappings_account_not_declared_by_prices_account() {
        let other = Pubkey::new_unique();
        // A well-formed mappings account under the right owner, but not the
        // one the prices account points at.
        assert_eq!(
            read(
                config(),
                &PRICES_KEY,
                &SCOPE_PROGRAM,
                &mut reusd_prices_data(),
                &other,
                &SCOPE_PROGRAM,
                &mut reusd_mappings_data(),
                REUSD_TIMESTAMP as i64,
            ),
            Err(ProgramError::InvalidAccountData)
        );
    }

    #[test]
    fn rejects_malformed_accounts() {
        // Wrong discriminators.
        let mut bad_prices = reusd_prices_data();
        bad_prices[0] ^= 0xff;
        assert!(read_reusd(
            &mut bad_prices,
            &mut reusd_mappings_data(),
            REUSD_TIMESTAMP as i64
        )
        .is_err());

        let mut bad_mappings = reusd_mappings_data();
        bad_mappings[0] ^= 0xff;
        assert!(read_reusd(
            &mut reusd_prices_data(),
            &mut bad_mappings,
            REUSD_TIMESTAMP as i64
        )
        .is_err());

        // Truncated accounts.
        let mut short_prices = reusd_prices_data();
        short_prices.truncate(ORACLE_PRICES_LEN - 1);
        assert!(read_reusd(
            &mut short_prices,
            &mut reusd_mappings_data(),
            REUSD_TIMESTAMP as i64
        )
        .is_err());

        let mut short_mappings = reusd_mappings_data();
        short_mappings.truncate(ORACLE_MAPPINGS_LEN - 1);
        assert!(read_reusd(
            &mut reusd_prices_data(),
            &mut short_mappings,
            REUSD_TIMESTAMP as i64
        )
        .is_err());
    }

    #[test]
    fn rejects_rebound_or_frozen_mapping_entry() {
        let now = REUSD_TIMESTAMP as i64;

        // Feed id rebound to another stream.
        let mut rebound = mappings_data(REUSD_INDEX, ORACLE_TYPE_CHAINLINK_EXCHANGE_RATE, [9; 32]);
        assert!(read_reusd(&mut reusd_prices_data(), &mut rebound, now).is_err());

        // Type rebound away from ChainlinkExchangeRate (e.g. Pyth = 0-adjacent
        // types); same feed id bytes.
        let mut retyped = mappings_data(REUSD_INDEX, 26, REUSD_FEED_ID);
        assert!(read_reusd(&mut reusd_prices_data(), &mut retyped, now).is_err());

        // Frozen entry: correct type and feed, frozen flag set.
        let mut frozen = mappings_data(
            REUSD_INDEX,
            ORACLE_TYPE_CHAINLINK_EXCHANGE_RATE | FROZEN_FLAG,
            REUSD_FEED_ID,
        );
        assert!(read_reusd(&mut reusd_prices_data(), &mut frozen, now).is_err());
    }

    #[test]
    fn rejects_non_positive_value_and_bad_exponent() {
        let now = REUSD_TIMESTAMP as i64;

        let mut zeroed = prices_data(&MAPPINGS_KEY, REUSD_INDEX, 0, REUSD_EXP, REUSD_TIMESTAMP);
        assert!(read_reusd(&mut zeroed, &mut reusd_mappings_data(), now).is_err());

        let mut bad_exp = prices_data(
            &MAPPINGS_KEY,
            REUSD_INDEX,
            REUSD_VALUE,
            MAX_EXP + 1,
            REUSD_TIMESTAMP,
        );
        assert!(read_reusd(&mut bad_exp, &mut reusd_mappings_data(), now).is_err());
    }

    #[test]
    fn rejects_out_of_range_index() {
        let bad_config = ScopeOracleConfig::new(
            SCOPE_PROGRAM.to_bytes(),
            PRICES_KEY.to_bytes(),
            REUSD_FEED_ID,
            ScopeOracleConfig::MAX_ENTRIES,
            300,
        );
        assert!(read(
            bad_config,
            &PRICES_KEY,
            &SCOPE_PROGRAM,
            &mut reusd_prices_data(),
            &MAPPINGS_KEY,
            &SCOPE_PROGRAM,
            &mut reusd_mappings_data(),
            REUSD_TIMESTAMP as i64,
        )
        .is_err());
    }

    #[test]
    fn parse_unverified_price_reads_entry_without_pinning() {
        let oracle = ScopeOracle::new(config());
        let price = oracle.parse_price(&reusd_prices_data()).unwrap();
        assert_eq!(
            price,
            OraclePrice {
                value: u128::from(REUSD_VALUE),
                decimals: 17,
            }
        );

        assert!(oracle.parse_price(&[0u8; 8]).is_none());
    }
}
