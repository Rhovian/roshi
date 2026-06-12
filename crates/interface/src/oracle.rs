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
}

impl OracleKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(kind: u8) -> Option<Self> {
        match kind {
            0 => Some(Self::Switchboard),
            1 => Some(Self::Pyth),
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

/// Oracle configuration stored by vault and asset accounts.
///
/// The serialized shape includes every supported oracle implementation from
/// the start. Switching implementations only changes `kind`, so account data
/// size remains stable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct OracleConfig {
    pub switchboard: SwitchboardOracleConfig,
    pub pyth: PythOracleConfig,
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
                if self.pyth.max_confidence_bps == 0 {
                    return Err(InvalidOracleConfig);
                }
                Ok(())
            }
            Ok(OracleKind::Switchboard) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub const fn switchboard(config: SwitchboardOracleConfig) -> Self {
        Self {
            switchboard: config,
            pyth: PythOracleConfig {
                feed_id: [0; 32],
                price_update_account: [0; 32],
                max_age_seconds: 0,
                max_confidence_bps: 0,
                price_decimals: 0,
                _padding: [0; 5],
            },
            kind: OracleKind::Switchboard.as_u8(),
            _padding: [0; 7],
        }
    }

    pub const fn pyth(config: PythOracleConfig) -> Self {
        Self {
            switchboard: SwitchboardOracleConfig {
                quote_account: [0; 32],
                queue_account: [0; 32],
                feed_id: [0; 32],
                max_age_slots: 0,
                price_decimals: 0,
                _padding: [0; 7],
            },
            pyth: config,
            kind: OracleKind::Pyth.as_u8(),
            _padding: [0; 7],
        }
    }

    pub const fn with_configs(
        kind: OracleKind,
        switchboard: SwitchboardOracleConfig,
        pyth: PythOracleConfig,
    ) -> Self {
        Self {
            switchboard,
            pyth,
            kind: kind.as_u8(),
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

    #[test]
    fn oracle_config_size_is_fixed_across_implementations() {
        let switchboard = OracleConfig::switchboard(SwitchboardOracleConfig::new(
            [1; 32], [2; 32], [3; 32], 6, 100,
        ));
        let pyth = OracleConfig::pyth(PythOracleConfig::new([4; 32], 8, 30, 250));

        assert_eq!(
            serialize(&switchboard).unwrap().len(),
            serialize(&pyth).unwrap().len()
        );
        assert_eq!(switchboard.kind(), Ok(OracleKind::Switchboard));
        assert_eq!(pyth.kind(), Ok(OracleKind::Pyth));
    }

    #[test]
    fn with_configs_keeps_inactive_config_available() {
        let switchboard_config = SwitchboardOracleConfig::new([1; 32], [2; 32], [3; 32], 6, 100);
        let pyth_config = PythOracleConfig::new([4; 32], 8, 30, 250);

        let config = OracleConfig::with_configs(OracleKind::Pyth, switchboard_config, pyth_config);

        assert_eq!(config.kind(), Ok(OracleKind::Pyth));
        assert_eq!(config.switchboard, switchboard_config);
        assert_eq!(config.pyth, pyth_config);
    }

    #[test]
    fn oracle_configs_are_zero_copy() {
        assert_zero_copy::<SwitchboardOracleConfig>();
        assert_zero_copy::<PythOracleConfig>();
        assert_zero_copy::<OracleConfig>();
        assert_eq!(core::mem::size_of::<SwitchboardOracleConfig>(), 112);
        assert_eq!(core::mem::size_of::<PythOracleConfig>(), 80);
        assert_eq!(core::mem::size_of::<OracleConfig>(), 200);
        assert_eq!(
            serialize(&OracleConfig::default()).unwrap().len(),
            core::mem::size_of::<OracleConfig>()
        );
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
        assert_eq!(switchboard.pyth.max_confidence_bps, 0);
        assert_eq!(switchboard.validate(), Ok(()));
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
