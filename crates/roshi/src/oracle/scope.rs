use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;

use super::{Oracle, OraclePrice, ScopeOracleConfig, ScopeOracleMapping};

// Kamino Scope account layout (Kamino-Finance/scope, Anchor zero-copy).
//
// `OraclePrices { oracle_mappings: Pubkey, prices: [DatedPrice; 512] }` with
// `DatedPrice { price: Price { value: u64, exp: u64 }, last_updated_slot: u64,
// unix_timestamp: u64, generic_data: [u8; 24] }` (56 bytes per entry).
//
// `OracleMappings` is a struct of arrays. Roshi commits the selected value
// from every array: price-info account, price type, TWAP source/reference
// tolerance, TWAP enabled bitmask, reference price, and generic data. The
// frozen flag occupies bit 7 of each live price type and is checked as runtime
// mapping state rather than stored in the commitment.
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
/// Offset of the `twap_source_or_ref_price_tolerance_bps` array.
pub const TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS_OFFSET: usize =
    PRICE_TYPES_OFFSET + ScopeOracleConfig::MAX_ENTRIES as usize;
/// Offset of the `twap_enabled_bitmask` array.
pub const TWAP_ENABLED_BITMASK_OFFSET: usize =
    TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS_OFFSET + 2 * ScopeOracleConfig::MAX_ENTRIES as usize;
/// Offset of the `ref_price` array.
pub const REF_PRICE_OFFSET: usize =
    TWAP_ENABLED_BITMASK_OFFSET + ScopeOracleConfig::MAX_ENTRIES as usize;
/// Offset of the `generic` array.
pub const GENERIC_OFFSET: usize = REF_PRICE_OFFSET + 2 * ScopeOracleConfig::MAX_ENTRIES as usize;
/// Exact `OracleMappings` account length: discriminator plus every mapping
/// array in Scope's zero-copy layout.
pub const ORACLE_MAPPINGS_LEN: usize =
    GENERIC_OFFSET + 20 * ScopeOracleConfig::MAX_ENTRIES as usize;

/// Canonical Kamino Scope mainnet program.
pub const SCOPE_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("HFn8GnPADiny6XqUoWE8uRPPxb29ikn4yTuPa9MF2fWJ");
/// Bit 7 of `price_types[i]` marks the entry frozen by the Scope admin.
pub const FROZEN_FLAG: u8 = 0x80;

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
    /// still matches the complete configured source mapping, and that the
    /// cached observation is positive, representable by [`OraclePrice`], not
    /// from the future, and within `max_age_seconds` of `unix_timestamp` (the
    /// current cluster time).
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
    /// still equal to the complete configured source mapping. Any Scope admin
    /// reconfiguration of that index makes the read fail loudly.
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

        let price_type = *mappings_data
            .get(PRICE_TYPES_OFFSET + index)
            .ok_or(ProgramError::InvalidAccountData)?;
        if price_type & FROZEN_FLAG != 0 {
            return Err(ProgramError::InvalidAccountData);
        }

        let mapping = ScopeOracleMapping::new(
            mapping_array_entry::<32>(mappings_data, PRICE_INFO_ACCOUNTS_OFFSET, index)
                .ok_or(ProgramError::InvalidAccountData)?,
            price_type & !FROZEN_FLAG,
            u16::from_le_bytes(
                mapping_array_entry::<2>(
                    mappings_data,
                    TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS_OFFSET,
                    index,
                )
                .ok_or(ProgramError::InvalidAccountData)?,
            ),
            mapping_array_entry::<1>(mappings_data, TWAP_ENABLED_BITMASK_OFFSET, index)
                .ok_or(ProgramError::InvalidAccountData)?[0],
            u16::from_le_bytes(
                mapping_array_entry::<2>(mappings_data, REF_PRICE_OFFSET, index)
                    .ok_or(ProgramError::InvalidAccountData)?,
            ),
            mapping_array_entry::<20>(mappings_data, GENERIC_OFFSET, index)
                .ok_or(ProgramError::InvalidAccountData)?,
        );
        if mapping != self.config.mapping {
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

/// Read one element from a fixed-width array embedded in Scope's
/// `OracleMappings` struct-of-arrays layout.
fn mapping_array_entry<const N: usize>(
    data: &[u8],
    offset: usize,
    index: usize,
) -> Option<[u8; N]> {
    let start = offset.checked_add(N.checked_mul(index)?)?;
    data.get(start..start.checked_add(N)?)?.try_into().ok()
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
    if entry.value == 0 {
        return None;
    }

    Some(OraclePrice {
        value: u128::from(entry.value),
        // Scope's exponent is value-dependent; it is taken from the entry on
        // every read, never assumed constant.
        decimals: u8::try_from(entry.exp).ok()?,
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
    const TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS: u16 = 9;
    const TWAP_ENABLED_BITMASK: u8 = 3;
    const REF_PRICE: u16 = 17;
    const GENERIC: [u8; 20] = [8; 20];
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
        twap_source_or_ref_price_tolerance_bps: [u16; 512],
        twap_enabled_bitmask: [u8; 512],
        ref_price: [u16; 512],
        generic: [[u8; 20]; 512],
    }

    /// Marker for fully initialized fixture layouts with no implicit padding.
    unsafe trait FixtureBytes {}

    // SAFETY: Every field is an integer, byte array, or `FixtureDatedPrice`;
    // their alignments divide their offsets and the final size exactly.
    unsafe impl FixtureBytes for FixtureOraclePrices {}
    // SAFETY: Each u16 array begins at an even offset, and the final size is a
    // multiple of the struct's two-byte alignment, so there is no implicit
    // padding.
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

    fn mapping() -> ScopeOracleMapping {
        ScopeOracleMapping::new(
            PRICE_INFO_ACCOUNT,
            PRICE_TYPE,
            TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS,
            TWAP_ENABLED_BITMASK,
            REF_PRICE,
            GENERIC,
        )
    }

    fn config() -> ScopeOracleConfig {
        ScopeOracleConfig::new(PRICES_KEY.to_bytes(), mapping(), PRICE_INDEX, 300)
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

    fn mappings_data(index: u16, mapping: ScopeOracleMapping, frozen: bool) -> Vec<u8> {
        assert_eq!(
            8 + core::mem::size_of::<FixtureOracleMappings>(),
            ORACLE_MAPPINGS_LEN
        );
        let mut fixture = FixtureOracleMappings {
            price_info_accounts: [[0; 32]; 512],
            price_types: [0; 512],
            twap_source_or_ref_price_tolerance_bps: [0; 512],
            twap_enabled_bitmask: [0; 512],
            ref_price: [0; 512],
            generic: [[0; 20]; 512],
        };
        let index = usize::from(index);
        fixture.price_info_accounts[index] = mapping.price_info_account;
        fixture.price_types[index] = mapping.price_type | if frozen { FROZEN_FLAG } else { 0 };
        fixture.twap_source_or_ref_price_tolerance_bps[index] =
            mapping.twap_source_or_ref_price_tolerance_bps;
        fixture.twap_enabled_bitmask[index] = mapping.twap_enabled_bitmask;
        fixture.ref_price[index] = mapping.ref_price;
        fixture.generic[index] = mapping.generic;
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
        mappings_data(PRICE_INDEX, mapping(), false)
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

    fn hex_bytes<const N: usize>(hex: &str) -> [u8; N] {
        assert_eq!(hex.len(), 2 * N);
        let mut bytes = [0; N];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[2 * index..2 * index + 2], 16).unwrap();
        }
        bytes
    }

    /// Captured from the canonical mainnet Scope accounts at index 445. All
    /// offsets and lengths in this fixture are numeric literals on purpose:
    /// the golden must fail if the production layout constants drift.
    #[test]
    fn decodes_captured_mainnet_scope_slices_at_independent_offsets() {
        let mut prices = vec![0xa5; 28_712];
        prices[..40].copy_from_slice(&hex_bytes::<40>(
            "598076dd0648b4923b5a811357e565086200fba2a64ee76c5c9068c72a2f6e9741361e19843a53b3",
        ));
        prices[24_960..25_016].copy_from_slice(&hex_bytes::<56>(
            "2e2ce9739dba8501110000000000000084be541a000000009e458e6a000000009e458e6a0000000000000000000000000000000000000000",
        ));

        let price_info_account =
            hex_bytes::<32>("00072c74e4a28e4cb93cf0551e1bd992af743db37ce791d65cccadd7556f5c40");
        let mut mappings = vec![0xa5; 29_704];
        mappings[..8].copy_from_slice(&[40, 244, 110, 80, 255, 214, 243, 188]);
        mappings[14_248..14_280].copy_from_slice(&price_info_account);
        mappings[16_837] = 38;
        mappings[17_794..17_796].copy_from_slice(&[0xf4, 0x01]);
        mappings[18_373] = 1;
        mappings[19_330..19_332].copy_from_slice(&[0xfc, 0x00]);
        mappings[28_364..28_384].copy_from_slice(&[0; 20]);

        let config = ScopeOracleConfig::new(
            PRICES_KEY.to_bytes(),
            ScopeOracleMapping::new(price_info_account, 38, 500, 1, 252, [0; 20]),
            445,
            300,
        );
        let price = read(
            config,
            &PRICES_KEY,
            &SCOPE_PROGRAM_ID,
            &mut prices,
            &MAPPINGS_KEY,
            &SCOPE_PROGRAM_ID,
            &mut mappings,
            1_787_708_890,
        )
        .unwrap();

        assert_eq!(
            price,
            OraclePrice {
                value: 109_698_951_357_738_030,
                decimals: 17,
            }
        );
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
    fn rejects_reconfigured_or_frozen_mapping_entry() {
        let now = PRICE_TIMESTAMP as i64;

        let mut reconfigured = Vec::new();
        let mut changed = mapping();
        changed.price_info_account = [9; 32];
        reconfigured.push(changed);
        changed = mapping();
        changed.price_type += 1;
        reconfigured.push(changed);
        changed = mapping();
        changed.twap_source_or_ref_price_tolerance_bps += 1;
        reconfigured.push(changed);
        changed = mapping();
        changed.twap_enabled_bitmask += 1;
        reconfigured.push(changed);
        changed = mapping();
        changed.ref_price += 1;
        reconfigured.push(changed);
        changed = mapping();
        changed.generic[0] ^= 1;
        reconfigured.push(changed);

        for changed in reconfigured {
            let mut mappings = mappings_data(PRICE_INDEX, changed, false);
            assert!(read_scope(&mut scope_prices_data(), &mut mappings, now).is_err());
        }

        // Frozen entry: correct source binding with the frozen flag set.
        let mut frozen = mappings_data(PRICE_INDEX, mapping(), true);
        assert!(read_scope(&mut scope_prices_data(), &mut frozen, now).is_err());
    }

    #[test]
    fn rejects_non_positive_value_and_unrepresentable_exponent() {
        let now = PRICE_TIMESTAMP as i64;

        let mut zeroed = prices_data(&MAPPINGS_KEY, PRICE_INDEX, 0, PRICE_EXP, PRICE_TIMESTAMP);
        assert!(read_scope(&mut zeroed, &mut scope_mappings_data(), now).is_err());

        // Scope source types are not all capped to 18 decimals. The reader
        // preserves any exponent representable by OraclePrice; downstream
        // arithmetic fails loudly if a particular operation cannot scale it.
        let mut larger_valid_exp =
            prices_data(&MAPPINGS_KEY, PRICE_INDEX, PRICE_VALUE, 19, PRICE_TIMESTAMP);
        assert_eq!(
            read_scope(&mut larger_valid_exp, &mut scope_mappings_data(), now)
                .unwrap()
                .decimals,
            19
        );

        let mut largest_valid_exp = prices_data(
            &MAPPINGS_KEY,
            PRICE_INDEX,
            PRICE_VALUE,
            u64::from(u8::MAX),
            PRICE_TIMESTAMP,
        );
        assert_eq!(
            read_scope(&mut largest_valid_exp, &mut scope_mappings_data(), now)
                .unwrap()
                .decimals,
            u8::MAX
        );

        let mut bad_exp = prices_data(
            &MAPPINGS_KEY,
            PRICE_INDEX,
            PRICE_VALUE,
            u64::from(u8::MAX) + 1,
            PRICE_TIMESTAMP,
        );
        assert!(read_scope(&mut bad_exp, &mut scope_mappings_data(), now).is_err());
    }

    #[test]
    fn rejects_out_of_range_index() {
        let bad_config = ScopeOracleConfig::new(
            PRICES_KEY.to_bytes(),
            mapping(),
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
        let config = ScopeOracleConfig::new(PRICES_KEY.to_bytes(), mapping(), last, 300);
        let price = read(
            config,
            &PRICES_KEY,
            &SCOPE_PROGRAM_ID,
            &mut prices_data(&MAPPINGS_KEY, last, PRICE_VALUE, PRICE_EXP, PRICE_TIMESTAMP),
            &MAPPINGS_KEY,
            &SCOPE_PROGRAM_ID,
            &mut mappings_data(last, mapping(), false),
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
