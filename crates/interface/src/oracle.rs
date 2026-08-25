use wincode::{SchemaRead, SchemaWrite};

/// A fixed-point oracle price: `value / 10^decimals` quote units per one
/// *whole* token of the priced asset (standard market convention).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OraclePrice {
    pub value: u128,
    pub decimals: u8,
}

impl OraclePrice {
    /// The exact price of the base asset in itself. Direct asset/base feeds
    /// price against this as their base leg, collapsing the two-leg
    /// conversion to a single feed.
    pub const UNIT: Self = Self {
        value: 1,
        decimals: 0,
    };
}

/// Discriminator for oracle implementations.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
#[wincode(tag_encoding = "u8")]
pub enum OracleKind {
    #[wincode(tag = 0)]
    Switchboard = 0,
    #[wincode(tag = 1)]
    Pyth = 1,
    #[wincode(tag = 2)]
    Scope = 2,
}

impl OracleKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(kind: u8) -> Option<Self> {
        match kind {
            0 => Some(Self::Switchboard),
            1 => Some(Self::Pyth),
            2 => Some(Self::Scope),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidOracleConfig;

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
/// [`OracleConfig::validate`] rejects an unbounded confidence interval. The
/// raw reader still treats `0` as "no width check" for inactive configs.
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

/// Kamino Scope oracle configuration stored with the asset it prices.
///
/// Scope ingests prices from multiple oracle sources and caches them in its
/// `OraclePrices` account, so Roshi only reads: `prices_account` pins that
/// account and `price_index` selects the entry. `price_type` and
/// `price_info_account` pin the selected entry's source mapping, so an admin
/// rebinding of the index fails loudly. The reader requires both Scope
/// accounts to be owned by the canonical Kamino Scope mainnet program.
///
/// There is no `price_decimals`: Scope stores a value-dependent exponent
/// (it maximizes precision, exponent <= 18), which the reader takes from the
/// entry on every read.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead,
)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct ScopeOracleConfig {
    pub prices_account: [u8; 32],
    pub price_info_account: [u8; 32],
    pub max_age_seconds: u64,
    pub price_index: u16,
    pub price_type: u8,
    _padding: [u8; 5],
}

impl ScopeOracleConfig {
    /// Entries in a Scope `OraclePrices` account (`MAX_ENTRIES` in
    /// Kamino-Finance/scope). `price_index` must be below this.
    pub const MAX_ENTRIES: u16 = 512;
    /// Scope reserves the high bit of a mapping's price type as its frozen
    /// flag. Configurations identify the underlying type without that flag.
    pub const MAX_PRICE_TYPE: u8 = 0x7f;

    pub const fn new(
        prices_account: [u8; 32],
        price_info_account: [u8; 32],
        price_type: u8,
        price_index: u16,
        max_age_seconds: u64,
    ) -> Self {
        Self {
            prices_account,
            price_info_account,
            max_age_seconds,
            price_index,
            price_type,
            _padding: [0; 5],
        }
    }
}

/// Size of the [`OracleConfig`] leg region shared by all implementations.
const LEGS_SIZE: usize = 192;
/// Historical field offsets of the two inline legs, preserved so serialized
/// configs written before the leg region became an explicit union decode
/// unchanged.
const SWITCHBOARD_LEG_OFFSET: usize = 0;
const PYTH_LEG_OFFSET: usize = 112;
/// Scope uses the start of the region when active.
const SCOPE_LEG_OFFSET: usize = 0;

/// Copy `bytes` into the leg region at `offset` (const-fn array copy).
const fn write_leg<const N: usize>(
    mut legs: [u8; LEGS_SIZE],
    offset: usize,
    bytes: [u8; N],
) -> [u8; LEGS_SIZE] {
    let mut index = 0;
    while index < N {
        legs[offset + index] = bytes[index];
        index += 1;
    }
    legs
}

/// Copy a leg's bytes out of the region at `offset` (const-fn array copy).
const fn read_leg<const N: usize>(legs: &[u8; LEGS_SIZE], offset: usize) -> [u8; N] {
    let mut bytes = [0u8; N];
    let mut index = 0;
    while index < N {
        bytes[index] = legs[offset + index];
        index += 1;
    }
    bytes
}

impl SwitchboardOracleConfig {
    const fn to_leg_bytes(self) -> [u8; 112] {
        // SAFETY: `repr(C)` fixes this byte layout:
        //
        // Bytes    Field            Type
        // 0..32    quote_account    [u8; 32]
        // 32..64   queue_account    [u8; 32]
        // 64..96   feed_id          [u8; 32]
        // 96..104  max_age_slots    u64
        // 104..105 price_decimals   u8
        // 105..112 _padding         [u8; 7]
        //
        // `max_age_slots` begins at an 8-byte boundary, and `_padding` extends
        // the initialized fields to the struct's 112-byte aligned size. There
        // is no implicit or potentially uninitialized padding.
        unsafe { core::mem::transmute(self) }
    }

    const fn from_leg_bytes(bytes: [u8; 112]) -> Self {
        // SAFETY: The exact layout is above; every field type accepts every bit pattern.
        unsafe { core::mem::transmute(bytes) }
    }
}

impl PythOracleConfig {
    const fn to_leg_bytes(self) -> [u8; 80] {
        // SAFETY: `repr(C)` fixes this byte layout:
        //
        // Bytes   Field                   Type
        // 0..32   feed_id                 [u8; 32]
        // 32..64  price_update_account    [u8; 32]
        // 64..72  max_age_seconds         u64
        // 72..74  max_confidence_bps      u16
        // 74..75  price_decimals          u8
        // 75..80  _padding                [u8; 5]
        //
        // The integer fields begin at their required alignments, and
        // `_padding` extends the initialized fields to the struct's 80-byte
        // aligned size. There is no implicit or uninitialized padding.
        unsafe { core::mem::transmute(self) }
    }

    const fn from_leg_bytes(bytes: [u8; 80]) -> Self {
        // SAFETY: The exact layout is above; every field type accepts every bit pattern.
        unsafe { core::mem::transmute(bytes) }
    }
}

impl ScopeOracleConfig {
    const fn to_leg_bytes(self) -> [u8; 80] {
        // SAFETY: `repr(C)` fixes this byte layout:
        //
        // Bytes   Field                 Type
        // 0..32   prices_account        [u8; 32]
        // 32..64  price_info_account    [u8; 32]
        // 64..72  max_age_seconds       u64
        // 72..74  price_index           u16
        // 74..75  price_type            u8
        // 75..80  _padding              [u8; 5]
        //
        // The integer fields begin at their required alignments, and
        // `_padding` extends the initialized fields to the struct's 80-byte
        // aligned size. There is no implicit or uninitialized padding.
        unsafe { core::mem::transmute(self) }
    }

    const fn from_leg_bytes(bytes: [u8; 80]) -> Self {
        // SAFETY: The exact layout is above; every field type accepts every bit pattern.
        unsafe { core::mem::transmute(bytes) }
    }
}

/// Oracle configuration stored by vault and asset accounts.
///
/// The serialized shape is a fixed-size leg region tagged by `kind`, so
/// switching implementations only changes `kind` and account data size never
/// changes. Each kind's configuration occupies a fixed sub-range of the
/// region: Switchboard at `0..112` and Pyth at `112..192` (the historical
/// field layout, byte-for-byte), Scope at `0..80`. Bytes outside the active
/// kind's sub-range are dead; constructors zero them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct OracleConfig {
    #[codama(type = fixed_size(number(u8), 192))]
    legs: [u8; LEGS_SIZE],
    kind: u8,
    _padding: [u8; 7],
}

impl OracleConfig {
    pub const fn raw_kind(&self) -> u8 {
        self.kind
    }

    pub const fn kind(&self) -> Result<OracleKind, InvalidOracleConfig> {
        match OracleKind::from_u8(self.kind) {
            Some(kind) => Ok(kind),
            None => Err(InvalidOracleConfig),
        }
    }

    pub const fn validate(&self) -> Result<(), InvalidOracleConfig> {
        match self.kind() {
            // An active Pyth leg must carry a confidence-width guardrail: an
            // unbounded confidence interval admits an arbitrarily uncertain,
            // technically-fresh price. Only the active leg is checked, so
            // zeroed inactive configs stay legal.
            Ok(OracleKind::Pyth) => {
                if self.pyth_config().max_confidence_bps == 0 {
                    return Err(InvalidOracleConfig);
                }
                Ok(())
            }
            Ok(OracleKind::Switchboard) => Ok(()),
            // An active Scope leg must address a real entry and identify an
            // underlying price type rather than mapping state. Everything
            // else fails closed at read time (owner, address, source mapping,
            // and freshness checks).
            Ok(OracleKind::Scope) => {
                let config = self.scope_config();
                if config.price_index >= ScopeOracleConfig::MAX_ENTRIES
                    || config.price_type > ScopeOracleConfig::MAX_PRICE_TYPE
                {
                    return Err(InvalidOracleConfig);
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// The Switchboard view of the leg region. Meaningful when `kind` is
    /// [`OracleKind::Switchboard`]; under other kinds it reads whatever bytes
    /// the active leg wrote.
    pub const fn switchboard_config(&self) -> SwitchboardOracleConfig {
        SwitchboardOracleConfig::from_leg_bytes(read_leg(&self.legs, SWITCHBOARD_LEG_OFFSET))
    }

    /// The Pyth view of the leg region. Meaningful when `kind` is
    /// [`OracleKind::Pyth`]; under other kinds it reads whatever bytes the
    /// active leg wrote.
    pub const fn pyth_config(&self) -> PythOracleConfig {
        PythOracleConfig::from_leg_bytes(read_leg(&self.legs, PYTH_LEG_OFFSET))
    }

    /// The Scope view of the leg region. Meaningful when `kind` is
    /// [`OracleKind::Scope`]; under other kinds it reads whatever bytes the
    /// active leg wrote.
    pub const fn scope_config(&self) -> ScopeOracleConfig {
        ScopeOracleConfig::from_leg_bytes(read_leg(&self.legs, SCOPE_LEG_OFFSET))
    }

    pub const fn switchboard(config: SwitchboardOracleConfig) -> Self {
        Self {
            legs: write_leg(
                [0; LEGS_SIZE],
                SWITCHBOARD_LEG_OFFSET,
                config.to_leg_bytes(),
            ),
            kind: OracleKind::Switchboard.as_u8(),
            _padding: [0; 7],
        }
    }

    pub const fn pyth(config: PythOracleConfig) -> Self {
        Self {
            legs: write_leg([0; LEGS_SIZE], PYTH_LEG_OFFSET, config.to_leg_bytes()),
            kind: OracleKind::Pyth.as_u8(),
            _padding: [0; 7],
        }
    }

    pub const fn scope(config: ScopeOracleConfig) -> Self {
        Self {
            legs: write_leg([0; LEGS_SIZE], SCOPE_LEG_OFFSET, config.to_leg_bytes()),
            kind: OracleKind::Scope.as_u8(),
            _padding: [0; 7],
        }
    }
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self::switchboard(SwitchboardOracleConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincode::{config::DefaultConfig, serialize, SchemaRead, SchemaWrite, TypeMeta};

    fn assert_zero_copy<T>()
    where
        T: wincode::ZeroCopy,
        T: for<'de> SchemaRead<'de, DefaultConfig> + SchemaWrite<DefaultConfig>,
    {
        assert_eq!(
            <T as SchemaRead<'_, DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
        assert_eq!(
            <T as SchemaWrite<DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
    }

    fn scope_config() -> ScopeOracleConfig {
        ScopeOracleConfig::new([6; 32], [7; 32], 26, 445, 300)
    }

    fn legacy_pyth_config(
        switchboard: SwitchboardOracleConfig,
        pyth: PythOracleConfig,
    ) -> OracleConfig {
        let legs = write_leg(
            [0; LEGS_SIZE],
            SWITCHBOARD_LEG_OFFSET,
            switchboard.to_leg_bytes(),
        );
        OracleConfig {
            legs: write_leg(legs, PYTH_LEG_OFFSET, pyth.to_leg_bytes()),
            kind: OracleKind::Pyth.as_u8(),
            _padding: [0; 7],
        }
    }

    #[test]
    fn oracle_config_size_is_fixed_across_implementations() {
        let switchboard = OracleConfig::switchboard(SwitchboardOracleConfig::new(
            [1; 32], [2; 32], [3; 32], 6, 100,
        ));
        let pyth = OracleConfig::pyth(PythOracleConfig::new([4; 32], 8, 30, 250));
        let scope = OracleConfig::scope(scope_config());

        assert_eq!(
            serialize(&switchboard).unwrap().len(),
            serialize(&pyth).unwrap().len()
        );
        assert_eq!(
            serialize(&pyth).unwrap().len(),
            serialize(&scope).unwrap().len()
        );
        assert_eq!(switchboard.kind(), Ok(OracleKind::Switchboard));
        assert_eq!(pyth.kind(), Ok(OracleKind::Pyth));
        assert_eq!(scope.kind(), Ok(OracleKind::Scope));
    }

    #[test]
    fn legacy_inline_layout_keeps_inactive_config_available() {
        let switchboard_config = SwitchboardOracleConfig::new([1; 32], [2; 32], [3; 32], 6, 100);
        let pyth_config = PythOracleConfig::new([4; 32], 8, 30, 250);

        let config = legacy_pyth_config(switchboard_config, pyth_config);

        assert_eq!(config.kind(), Ok(OracleKind::Pyth));
        assert_eq!(config.switchboard_config(), switchboard_config);
        assert_eq!(config.pyth_config(), pyth_config);
    }

    #[test]
    fn oracle_configs_are_zero_copy() {
        assert_zero_copy::<SwitchboardOracleConfig>();
        assert_zero_copy::<PythOracleConfig>();
        assert_zero_copy::<ScopeOracleConfig>();
        assert_zero_copy::<OracleConfig>();
        assert_eq!(core::mem::size_of::<SwitchboardOracleConfig>(), 112);
        assert_eq!(core::mem::size_of::<PythOracleConfig>(), 80);
        assert_eq!(core::mem::size_of::<ScopeOracleConfig>(), 80);
        assert_eq!(core::mem::size_of::<OracleConfig>(), 200);
        assert_eq!(
            serialize(&OracleConfig::default()).unwrap().len(),
            core::mem::size_of::<OracleConfig>()
        );
    }

    /// The leg region must keep the exact byte layout of the historical
    /// `{ switchboard, pyth, kind, _padding }` field struct: each inline leg's
    /// serialized bytes at its historical offset, `kind` at 192. Existing
    /// on-chain vault and asset accounts depend on this.
    #[test]
    fn oracle_config_layout_matches_legacy_leg_fields() {
        let switchboard_config =
            SwitchboardOracleConfig::new([1; 32], [2; 32], [3; 32], 6, 0x0102_0304_0506_0708);
        let pyth_config = PythOracleConfig::new([4; 32], 8, 0x1112_1314_1516_1718, 250)
            .pin_price_update_account([9; 32]);

        let config = legacy_pyth_config(switchboard_config, pyth_config);

        let mut expected = Vec::new();
        expected.extend_from_slice(&serialize(&switchboard_config).unwrap());
        expected.extend_from_slice(&serialize(&pyth_config).unwrap());
        expected.push(OracleKind::Pyth.as_u8());
        expected.extend_from_slice(&[0; 7]);

        assert_eq!(serialize(&config).unwrap(), expected);
    }

    #[test]
    fn scope_config_round_trips_through_leg_region() {
        let config = OracleConfig::scope(scope_config());

        assert_eq!(config.kind(), Ok(OracleKind::Scope));
        assert_eq!(config.scope_config(), scope_config());
        assert_eq!(config.validate(), Ok(()));

        let bytes = serialize(&config).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(&serialize(&scope_config()).unwrap());
        expected.extend_from_slice(&[0; 112]);
        expected.push(OracleKind::Scope.as_u8());
        expected.extend_from_slice(&[0; 7]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn pyth_price_update_pin_defaults_off_and_round_trips() {
        let unpinned = PythOracleConfig::new([4; 32], 8, 30, 250);
        assert_eq!(unpinned.pinned_price_update_account(), None);

        let pinned = unpinned.pin_price_update_account([5; 32]);
        assert_eq!(pinned.pinned_price_update_account(), Some([5; 32]));
    }

    #[test]
    fn validate_requires_confidence_bound_on_active_pyth_leg() {
        let unbounded = OracleConfig::pyth(PythOracleConfig::new([4; 32], 8, 30, 0));
        assert_eq!(unbounded.validate(), Err(InvalidOracleConfig));

        let bounded = OracleConfig::pyth(PythOracleConfig::new([4; 32], 8, 30, 250));
        assert_eq!(bounded.validate(), Ok(()));

        // The inactive Pyth config may stay zeroed under a Switchboard kind.
        let switchboard = OracleConfig::switchboard(SwitchboardOracleConfig::new(
            [1; 32], [2; 32], [3; 32], 6, 100,
        ));
        assert_eq!(switchboard.pyth_config().max_confidence_bps, 0);
        assert_eq!(switchboard.validate(), Ok(()));
    }

    #[test]
    fn validate_requires_in_range_scope_index() {
        let out_of_range = OracleConfig::scope(ScopeOracleConfig::new(
            [6; 32],
            [7; 32],
            26,
            ScopeOracleConfig::MAX_ENTRIES,
            300,
        ));
        assert_eq!(out_of_range.validate(), Err(InvalidOracleConfig));

        let last_entry = OracleConfig::scope(ScopeOracleConfig::new(
            [6; 32],
            [7; 32],
            26,
            ScopeOracleConfig::MAX_ENTRIES - 1,
            300,
        ));
        assert_eq!(last_entry.validate(), Ok(()));

        let frozen_type = OracleConfig::scope(ScopeOracleConfig::new(
            [6; 32],
            [7; 32],
            0x80,
            ScopeOracleConfig::MAX_ENTRIES - 1,
            300,
        ));
        assert_eq!(frozen_type.validate(), Err(InvalidOracleConfig));
    }

    #[test]
    fn oracle_config_rejects_invalid_kind() {
        let config = OracleConfig {
            kind: 255,
            ..OracleConfig::default()
        };

        assert_eq!(config.kind(), Err(InvalidOracleConfig));
        assert_eq!(config.validate(), Err(InvalidOracleConfig));
    }
}
