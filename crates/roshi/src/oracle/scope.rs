use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;

use super::{Oracle, OraclePrice, ScopeOracleConfig};

// Kamino Scope account layout (Kamino-Finance/scope, Anchor zero-copy).
//
// `OraclePrices { oracle_mappings: Pubkey, prices: [DatedPrice; 512] }` with
// `DatedPrice { price: Price { value: u64, exp: u64 }, last_updated_slot: u64,
// unix_timestamp: u64, generic_data: [u8; 24] }` (56 bytes per entry).
//
// `OracleMappings` is a struct of arrays; the two Roshi reads are
// `price_info_accounts: [Pubkey; 512]` and `price_types: [u8; 512]`, with the
// frozen flag in bit 7 of each price type.
//
// These constants are the one in-tree encoding of that layout; test and fuzz
// account builders import them rather than restating the numbers.

/// Anchor discriminator of Scope's `OraclePrices` account.
pub const ORACLE_PRICES_DISCRIMINATOR: &[u8; 8] = &[89, 128, 118, 221, 6, 72, 180, 146];
/// Offset of the declared `oracle_mappings` pubkey inside `OraclePrices`.
pub const ORACLE_PRICES_MAPPINGS_OFFSET: usize = 8;
/// Offset of the `prices` array inside `OraclePrices`.
pub const DATED_PRICES_OFFSET: usize = 40;
/// Stride of one `DatedPrice` entry.
pub const DATED_PRICE_SIZE: usize = 56;
/// Exact `OraclePrices` account length: header plus the full entry array.
pub const ORACLE_PRICES_LEN: usize =
    DATED_PRICES_OFFSET + DATED_PRICE_SIZE * ScopeOracleConfig::MAX_ENTRIES as usize;

/// Anchor discriminator of Scope's `OracleMappings` account.
pub const ORACLE_MAPPINGS_DISCRIMINATOR: &[u8; 8] = &[40, 244, 110, 80, 255, 214, 243, 188];
/// Offset of the `price_info_accounts` array inside `OracleMappings`.
pub const PRICE_INFO_ACCOUNTS_OFFSET: usize = 8;
/// Offset of the `price_types` array, directly after `price_info_accounts`.
pub const PRICE_TYPES_OFFSET: usize =
    PRICE_INFO_ACCOUNTS_OFFSET + 32 * ScopeOracleConfig::MAX_ENTRIES as usize;
/// Exact `OracleMappings` account length. Unlike the prices account this is
/// not derivable from the arrays Roshi reads: the account carries further
/// unread arrays whose observed total is pinned here.
pub const ORACLE_MAPPINGS_LEN: usize = 29_704;

/// Canonical Kamino Scope mainnet program.
pub const SCOPE_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("HFn8GnPADiny6XqUoWE8uRPPxb29ikn4yTuPa9MF2fWJ");
/// Bit 7 of `price_types[i]` marks the entry frozen by the Scope admin.
pub const FROZEN_FLAG: u8 = 0x80;

/// Scope exponents never exceed 18 (`decimal_to_price` caps them); a larger
/// value is corrupted or foreign data.
const MAX_EXP: u64 = 18;

/// Kamino Scope cached-price reader.
///
/// Scope owns source ingestion and caches normalized values in its
/// `OraclePrices` account. Roshi only reads: it pins the prices account, its
/// canonical mainnet program owner, the mappings account the prices account
/// itself declares, and the entry's configured source binding on every read.
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
        if !well_formed(data, ORACLE_PRICES_LEN, ORACLE_PRICES_DISCRIMINATOR) {
            return None;
        }

        let entry = DatedPrice::parse(data, self.config.price_index)?;
        price_from_entry(&entry)
    }

    /// Read the configured entry as a fully verified price.
    ///
    /// Checks, in order: the prices account address and owner pin, both
    /// account discriminators and lengths, that `mappings_account` is the one
    /// the prices account declares, that the mapping entry is unfrozen and
    /// still bound to the configured price type and price-info account, and
    /// that the cached observation is positive, sanely scaled, not from the
    /// future, and within `max_age_seconds` of `unix_timestamp` (the current
    /// cluster time).
    pub fn read_verified_price(
        &self,
        prices_account: &AccountInfo,
        mappings_account: &AccountInfo,
        unix_timestamp: i64,
    ) -> Result<OraclePrice, ProgramError> {
        if prices_account.key.to_bytes() != self.config.prices_account {
            return Err(ProgramError::InvalidAccountData);
        }
        if prices_account.owner != &SCOPE_PROGRAM_ID || mappings_account.owner != &SCOPE_PROGRAM_ID
        {
            return Err(ProgramError::IllegalOwner);
        }

        let prices_data = prices_account.data.borrow();
        if !well_formed(&prices_data, ORACLE_PRICES_LEN, ORACLE_PRICES_DISCRIMINATOR) {
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

    /// Require the mapping entry at the configured index to be unfrozen and
    /// still bound to the configured source. A Scope admin rebinding of the
    /// index changes its price type or price-info account and the read fails
    /// loudly.
    fn verify_mapping_entry(&self, mappings_data: &[u8]) -> Result<(), ProgramError> {
        if !well_formed(
            mappings_data,
            ORACLE_MAPPINGS_LEN,
            ORACLE_MAPPINGS_DISCRIMINATOR,
        ) {
            return Err(ProgramError::InvalidAccountData);
        }

        // The single runtime bound on the configured index; together with the
        // exact-length gate above it makes the indexing below in-bounds.
        // `OracleConfig::validate` already rejects such configs at every
        // account load, so this is only reachable through a hand-built config.
        if self.config.price_index >= ScopeOracleConfig::MAX_ENTRIES {
            return Err(ProgramError::InvalidAccountData);
        }
        let index = usize::from(self.config.price_index);

        let price_type = mappings_data[PRICE_TYPES_OFFSET + index];
        if price_type & FROZEN_FLAG != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        if price_type & !FROZEN_FLAG != self.config.price_type {
            return Err(ProgramError::InvalidAccountData);
        }

        let price_info_offset = PRICE_INFO_ACCOUNTS_OFFSET + 32 * index;
        if mappings_data[price_info_offset..price_info_offset + 32]
            != self.config.price_info_account
        {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(())
    }

    /// Freshness is judged on the entry's `unix_timestamp`, never the read
    /// time.
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

/// A well-formed Scope account: the pinned exact length and the Anchor
/// discriminator.
fn well_formed(data: &[u8], len: usize, discriminator: &[u8; 8]) -> bool {
    data.len() == len && data.get(..8) == Some(discriminator.as_slice())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DatedPrice {
    value: u64,
    exp: u64,
    unix_timestamp: u64,
}

impl DatedPrice {
    /// Read the entry at `price_index` from `OraclePrices` account bytes the
    /// caller has already length-checked. An out-of-range index lands past the
    /// exact account length and yields `None` through the checked reads — no
    /// separate bound check is needed.
    fn parse(data: &[u8], price_index: u16) -> Option<Self> {
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
    use super::*;

    const PRICES_KEY: Pubkey =
        solana_pubkey::pubkey!("3t4JZcueEzTbVP6kLxXrL3VpWx45jDer4eqysweBchNH");
    const MAPPINGS_KEY: Pubkey =
        solana_pubkey::pubkey!("4zh6bmb77qX2CL7t5AJYCqa6YqFafbz3QJNeFvZjLowg");

    const PRICE_INFO_ACCOUNT: [u8; 32] = [7; 32];
    const PRICE_TYPE: u8 = 26;
    const PRICE_INDEX: u16 = 445;
    const PRICE_VALUE: u64 = 109_679_858_068_090_600;
    const PRICE_EXP: u64 = 17;
    const PRICE_TIMESTAMP: u64 = 1_787_619_070;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct FixturePrice {
        value: u64,
        exp: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct FixtureDatedPrice {
        price: FixturePrice,
        last_updated_slot: u64,
        unix_timestamp: u64,
        generic_data: [u8; 24],
    }

    #[repr(C)]
    struct FixtureOraclePrices {
        oracle_mappings: [u8; 32],
        prices: [FixtureDatedPrice; 512],
    }

    #[repr(C)]
    struct FixtureOracleMappings {
        price_info_accounts: [[u8; 32]; 512],
        price_types: [u8; 512],
        unread_suffix: [u8; 12_800],
    }

    /// Marker for fully initialized fixture layouts with no implicit padding.
    unsafe trait FixtureBytes {}

    // SAFETY: Every field is an integer, byte array, or `FixtureDatedPrice`;
    // their alignments divide their offsets and the final size exactly.
    unsafe impl FixtureBytes for FixtureOraclePrices {}
    // SAFETY: Every field is a byte array, so the struct has alignment one and
    // cannot contain implicit padding.
    unsafe impl FixtureBytes for FixtureOracleMappings {}

    fn encode_fixture<T: FixtureBytes>(discriminator: &[u8; 8], fixture: &T) -> Vec<u8> {
        // SAFETY: `FixtureBytes` is implemented only for fully initialized
        // padding-free fixture layouts.
        let body = unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(fixture).cast::<u8>(),
                core::mem::size_of::<T>(),
            )
        };
        let mut data = Vec::with_capacity(8 + body.len());
        data.extend_from_slice(discriminator);
        data.extend_from_slice(body);
        data
    }

    fn config() -> ScopeOracleConfig {
        ScopeOracleConfig::new(
            PRICES_KEY.to_bytes(),
            PRICE_INFO_ACCOUNT,
            PRICE_TYPE,
            PRICE_INDEX,
            300,
        )
    }

    fn prices_data(mappings: &Pubkey, index: u16, value: u64, exp: u64, timestamp: u64) -> Vec<u8> {
        assert_eq!(core::mem::size_of::<FixtureDatedPrice>(), DATED_PRICE_SIZE);
        assert_eq!(
            8 + core::mem::size_of::<FixtureOraclePrices>(),
            ORACLE_PRICES_LEN
        );
        let mut fixture = FixtureOraclePrices {
            oracle_mappings: mappings.to_bytes(),
            prices: [FixtureDatedPrice::default(); 512],
        };
        fixture.prices[usize::from(index)] = FixtureDatedPrice {
            price: FixturePrice { value, exp },
            last_updated_slot: 0,
            unix_timestamp: timestamp,
            generic_data: [0; 24],
        };
        encode_fixture(ORACLE_PRICES_DISCRIMINATOR, &fixture)
    }

    fn mappings_data(index: u16, price_type: u8, price_info_account: [u8; 32]) -> Vec<u8> {
        assert_eq!(
            8 + core::mem::size_of::<FixtureOracleMappings>(),
            ORACLE_MAPPINGS_LEN
        );
        let mut fixture = FixtureOracleMappings {
            price_info_accounts: [[0; 32]; 512],
            price_types: [0; 512],
            unread_suffix: [0; 12_800],
        };
        fixture.price_info_accounts[usize::from(index)] = price_info_account;
        fixture.price_types[usize::from(index)] = price_type;
        encode_fixture(ORACLE_MAPPINGS_DISCRIMINATOR, &fixture)
    }

    fn scope_prices_data() -> Vec<u8> {
        prices_data(
            &MAPPINGS_KEY,
            PRICE_INDEX,
            PRICE_VALUE,
            PRICE_EXP,
            PRICE_TIMESTAMP,
        )
    }

    fn scope_mappings_data() -> Vec<u8> {
        mappings_data(PRICE_INDEX, PRICE_TYPE, PRICE_INFO_ACCOUNT)
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

    fn read_scope(
        prices_data: &mut [u8],
        mappings_data: &mut [u8],
        unix_timestamp: i64,
    ) -> Result<OraclePrice, ProgramError> {
        read(
            config(),
            &PRICES_KEY,
            &SCOPE_PROGRAM_ID,
            prices_data,
            &MAPPINGS_KEY,
            &SCOPE_PROGRAM_ID,
            mappings_data,
            unix_timestamp,
        )
    }

    #[test]
    fn decodes_scope_entry_fixture() {
        let price = read_scope(
            &mut scope_prices_data(),
            &mut scope_mappings_data(),
            PRICE_TIMESTAMP as i64 + 60,
        )
        .unwrap();

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
        let now = PRICE_TIMESTAMP as i64;

        // At the configured max age exactly: fresh.
        assert!(read_scope(
            &mut scope_prices_data(),
            &mut scope_mappings_data(),
            now + 300
        )
        .is_ok());
        // One second past: stale.
        assert!(read_scope(
            &mut scope_prices_data(),
            &mut scope_mappings_data(),
            now + 301
        )
        .is_err());
        // Observation from the future: rejected.
        assert!(read_scope(
            &mut scope_prices_data(),
            &mut scope_mappings_data(),
            now - 1
        )
        .is_err());
        assert!(read_scope(&mut scope_prices_data(), &mut scope_mappings_data(), -1).is_err());
    }

    #[test]
    fn rejects_wrong_prices_account_or_owner() {
        let other = Pubkey::new_unique();

        assert_eq!(
            read(
                config(),
                &other,
                &SCOPE_PROGRAM_ID,
                &mut scope_prices_data(),
                &MAPPINGS_KEY,
                &SCOPE_PROGRAM_ID,
                &mut scope_mappings_data(),
                PRICE_TIMESTAMP as i64,
            ),
            Err(ProgramError::InvalidAccountData)
        );
        assert_eq!(
            read(
                config(),
                &PRICES_KEY,
                &other,
                &mut scope_prices_data(),
                &MAPPINGS_KEY,
                &SCOPE_PROGRAM_ID,
                &mut scope_mappings_data(),
                PRICE_TIMESTAMP as i64,
            ),
            Err(ProgramError::IllegalOwner)
        );
        assert_eq!(
            read(
                config(),
                &PRICES_KEY,
                &SCOPE_PROGRAM_ID,
                &mut scope_prices_data(),
                &MAPPINGS_KEY,
                &other,
                &mut scope_mappings_data(),
                PRICE_TIMESTAMP as i64,
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
                &SCOPE_PROGRAM_ID,
                &mut scope_prices_data(),
                &other,
                &SCOPE_PROGRAM_ID,
                &mut scope_mappings_data(),
                PRICE_TIMESTAMP as i64,
            ),
            Err(ProgramError::InvalidAccountData)
        );
    }

    #[test]
    fn rejects_malformed_accounts() {
        // Wrong discriminators.
        let mut bad_prices = scope_prices_data();
        bad_prices[0] ^= 0xff;
        assert!(read_scope(
            &mut bad_prices,
            &mut scope_mappings_data(),
            PRICE_TIMESTAMP as i64
        )
        .is_err());

        let mut bad_mappings = scope_mappings_data();
        bad_mappings[0] ^= 0xff;
        assert!(read_scope(
            &mut scope_prices_data(),
            &mut bad_mappings,
            PRICE_TIMESTAMP as i64
        )
        .is_err());

        // Truncated accounts.
        let mut short_prices = scope_prices_data();
        short_prices.truncate(ORACLE_PRICES_LEN - 1);
        assert!(read_scope(
            &mut short_prices,
            &mut scope_mappings_data(),
            PRICE_TIMESTAMP as i64
        )
        .is_err());

        let mut short_mappings = scope_mappings_data();
        short_mappings.truncate(ORACLE_MAPPINGS_LEN - 1);
        assert!(read_scope(
            &mut scope_prices_data(),
            &mut short_mappings,
            PRICE_TIMESTAMP as i64
        )
        .is_err());

        let mut long_prices = scope_prices_data();
        long_prices.push(0);
        assert!(read_scope(
            &mut long_prices,
            &mut scope_mappings_data(),
            PRICE_TIMESTAMP as i64
        )
        .is_err());

        let mut long_mappings = scope_mappings_data();
        long_mappings.push(0);
        assert!(read_scope(
            &mut scope_prices_data(),
            &mut long_mappings,
            PRICE_TIMESTAMP as i64
        )
        .is_err());
    }

    #[test]
    fn rejects_rebound_or_frozen_mapping_entry() {
        let now = PRICE_TIMESTAMP as i64;

        // Price-info account rebound while retaining the configured type.
        let mut rebound = mappings_data(PRICE_INDEX, PRICE_TYPE, [9; 32]);
        assert!(read_scope(&mut scope_prices_data(), &mut rebound, now).is_err());

        // Type rebound while retaining the configured price-info account.
        let mut retyped = mappings_data(PRICE_INDEX, PRICE_TYPE + 1, PRICE_INFO_ACCOUNT);
        assert!(read_scope(&mut scope_prices_data(), &mut retyped, now).is_err());

        // Frozen entry: correct source binding with the frozen flag set.
        let mut frozen = mappings_data(PRICE_INDEX, PRICE_TYPE | FROZEN_FLAG, PRICE_INFO_ACCOUNT);
        assert!(read_scope(&mut scope_prices_data(), &mut frozen, now).is_err());
    }

    #[test]
    fn rejects_non_positive_value_and_bad_exponent() {
        let now = PRICE_TIMESTAMP as i64;

        let mut zeroed = prices_data(&MAPPINGS_KEY, PRICE_INDEX, 0, PRICE_EXP, PRICE_TIMESTAMP);
        assert!(read_scope(&mut zeroed, &mut scope_mappings_data(), now).is_err());

        let mut bad_exp = prices_data(
            &MAPPINGS_KEY,
            PRICE_INDEX,
            PRICE_VALUE,
            MAX_EXP + 1,
            PRICE_TIMESTAMP,
        );
        assert!(read_scope(&mut bad_exp, &mut scope_mappings_data(), now).is_err());
    }

    #[test]
    fn rejects_out_of_range_index() {
        let bad_config = ScopeOracleConfig::new(
            PRICES_KEY.to_bytes(),
            PRICE_INFO_ACCOUNT,
            PRICE_TYPE,
            ScopeOracleConfig::MAX_ENTRIES,
            300,
        );
        assert!(read(
            bad_config,
            &PRICES_KEY,
            &SCOPE_PROGRAM_ID,
            &mut scope_prices_data(),
            &MAPPINGS_KEY,
            &SCOPE_PROGRAM_ID,
            &mut scope_mappings_data(),
            PRICE_TIMESTAMP as i64,
        )
        .is_err());
    }

    #[test]
    fn reads_last_entry() {
        let last = ScopeOracleConfig::MAX_ENTRIES - 1;
        let config = ScopeOracleConfig::new(
            PRICES_KEY.to_bytes(),
            PRICE_INFO_ACCOUNT,
            PRICE_TYPE,
            last,
            300,
        );
        let price = read(
            config,
            &PRICES_KEY,
            &SCOPE_PROGRAM_ID,
            &mut prices_data(&MAPPINGS_KEY, last, PRICE_VALUE, PRICE_EXP, PRICE_TIMESTAMP),
            &MAPPINGS_KEY,
            &SCOPE_PROGRAM_ID,
            &mut mappings_data(last, PRICE_TYPE, PRICE_INFO_ACCOUNT),
            PRICE_TIMESTAMP as i64,
        )
        .unwrap();
        assert_eq!(price.value, u128::from(PRICE_VALUE));
    }

    #[test]
    fn parse_unverified_price_reads_entry_without_pinning() {
        let oracle = ScopeOracle::new(config());
        let price = oracle.parse_price(&scope_prices_data()).unwrap();
        assert_eq!(
            price,
            OraclePrice {
                value: u128::from(PRICE_VALUE),
                decimals: 17,
            }
        );

        assert!(oracle.parse_price(&[0u8; 8]).is_none());
    }
}
