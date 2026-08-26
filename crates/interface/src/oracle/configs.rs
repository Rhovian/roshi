use wincode::{SchemaRead, SchemaWrite};

/// Switchboard On-Demand oracle configuration stored with the asset it prices.
///
/// `price_decimals` is the scale of the raw oracle price. A price of `123`
/// with `price_decimals = 2` represents `1.23`.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead,
)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct SwitchboardOracleConfig {
    pub quote_account: [u8; 32],
    pub queue_account: [u8; 32],
    pub feed_id: [u8; 32],
    pub max_age_slots: u64,
    pub price_decimals: u8,
    _padding: [u8; 7],
}

impl SwitchboardOracleConfig {
    pub const fn new(
        quote_account: [u8; 32],
        queue_account: [u8; 32],
        feed_id: [u8; 32],
        price_decimals: u8,
        max_age_slots: u64,
    ) -> Self {
        Self {
            quote_account,
            queue_account,
            feed_id,
            max_age_slots,
            price_decimals,
            _padding: [0; 7],
        }
    }
}

/// Pyth pull-oracle configuration stored with the asset it prices.
///
/// `feed_id` is the 32-byte Pyth price feed id expected inside the submitted
/// price update account. `price_decimals` is the scale Roshi exposes through
/// `OraclePrice`; for example, a Pyth price of `123456789 * 10^-8` with
/// `price_decimals = 8` is returned as `123456789`.
///
/// `max_confidence_bps` must be nonzero for an active Pyth leg —
/// [`super::OracleConfig::validate`] rejects an unbounded confidence interval.
/// The raw reader still treats `0` as "no width check" for inactive configs.
///
/// `price_update_account` optionally pins the price update account by address;
/// all-zeros (the default) accepts any Pyth-verified update account carrying
/// `feed_id`, which is the intended pull-oracle posture.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead,
)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct PythOracleConfig {
    pub feed_id: [u8; 32],
    pub price_update_account: [u8; 32],
    pub max_age_seconds: u64,
    pub max_confidence_bps: u16,
    pub price_decimals: u8,
    _padding: [u8; 5],
}

impl PythOracleConfig {
    pub const fn new(
        feed_id: [u8; 32],
        price_decimals: u8,
        max_age_seconds: u64,
        max_confidence_bps: u16,
    ) -> Self {
        Self {
            feed_id,
            price_update_account: [0; 32],
            max_age_seconds,
            max_confidence_bps,
            price_decimals,
            _padding: [0; 5],
        }
    }

    /// Pin pricing to one specific price update account (e.g. a sponsored
    /// Pyth feed account) instead of accepting any verified update for
    /// `feed_id`.
    pub const fn pin_price_update_account(mut self, price_update_account: [u8; 32]) -> Self {
        self.price_update_account = price_update_account;
        self
    }

    /// The pinned price update account, or `None` when any verified update
    /// for `feed_id` is accepted (`price_update_account` all-zeros).
    pub fn pinned_price_update_account(&self) -> Option<[u8; 32]> {
        if self.price_update_account == [0; 32] {
            return None;
        }
        Some(self.price_update_account)
    }
}

/// The complete source mapping committed for one Kamino Scope price index.
///
/// Scope's `OracleMappings` account stores these fields in separate arrays;
/// this is a compact canonical commitment to the values selected from all of
/// those arrays at one index, not a wire representation of that account. The
/// `price_type` excludes Scope's high frozen bit, which is mapping state rather
/// than part of the underlying oracle type.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead,
)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct ScopeOracleMapping {
    pub price_info_account: [u8; 32],
    pub twap_source_or_ref_price_tolerance_bps: u16,
    pub ref_price: u16,
    pub generic: [u8; 20],
    pub price_type: u8,
    pub twap_enabled_bitmask: u8,
    _padding: [u8; 6],
}

impl ScopeOracleMapping {
    pub const fn new(
        price_info_account: [u8; 32],
        price_type: u8,
        twap_source_or_ref_price_tolerance_bps: u16,
        twap_enabled_bitmask: u8,
        ref_price: u16,
        generic: [u8; 20],
    ) -> Self {
        Self {
            price_info_account,
            twap_source_or_ref_price_tolerance_bps,
            ref_price,
            generic,
            price_type,
            twap_enabled_bitmask,
            _padding: [0; 6],
        }
    }

    pub(super) const fn has_canonical_padding(&self) -> bool {
        bytes_are_zero(&self._padding)
    }
}

/// Kamino Scope oracle configuration stored with the asset it prices.
///
/// Scope ingests prices from multiple oracle sources and caches them in its
/// `OraclePrices` account. `prices_account` pins that account and `price_index`
/// selects the entry; `mapping` pins the selected entry's source configuration.
/// There is no `price_decimals`: the reader takes Scope's value-dependent
/// exponent from the entry on every read.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead,
)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct ScopeOracleConfig {
    pub prices_account: [u8; 32],
    pub mapping: ScopeOracleMapping,
    pub max_age_seconds: u64,
    pub price_index: u16,
    _padding: [u8; 6],
}

impl ScopeOracleConfig {
    /// Entries in a Scope `OraclePrices` account (`MAX_ENTRIES` in Scope).
    pub const MAX_ENTRIES: u16 = 512;
    /// Scope reserves the high bit of a mapping's price type as its frozen flag.
    pub const MAX_PRICE_TYPE: u8 = 0x7f;

    pub const fn new(
        prices_account: [u8; 32],
        mapping: ScopeOracleMapping,
        price_index: u16,
        max_age_seconds: u64,
    ) -> Self {
        Self {
            prices_account,
            mapping,
            max_age_seconds,
            price_index,
            _padding: [0; 6],
        }
    }

    pub(super) const fn has_canonical_padding(&self) -> bool {
        bytes_are_zero(&self._padding) && self.mapping.has_canonical_padding()
    }
}

const fn bytes_are_zero(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}
