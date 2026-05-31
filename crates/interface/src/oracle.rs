use wincode::{SchemaRead, SchemaWrite};

/// Discriminator for oracle implementations.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
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

/// Switchboard On-Demand oracle configuration stored with the asset it prices.
///
/// `price_decimals` is the scale of the raw oracle price. A price of `123`
/// with `price_decimals = 2` represents `1.23`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct SwitchboardOracleConfig {
    pub quote_account: [u8; 32],
    pub queue_account: [u8; 32],
    pub feed_id: [u8; 32],
    pub price_decimals: u8,
    pub max_age_slots: u64,
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
            price_decimals,
            max_age_slots,
        }
    }
}

impl Default for SwitchboardOracleConfig {
    fn default() -> Self {
        Self {
            quote_account: [0; 32],
            queue_account: [0; 32],
            feed_id: [0; 32],
            price_decimals: 0,
            max_age_slots: 0,
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
/// `max_confidence_bps = 0` disables the confidence-width guardrail.
#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct PythOracleConfig {
    pub feed_id: [u8; 32],
    pub price_decimals: u8,
    pub max_age_seconds: u64,
    pub max_confidence_bps: u16,
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
            price_decimals,
            max_age_seconds,
            max_confidence_bps,
        }
    }
}

impl Default for PythOracleConfig {
    fn default() -> Self {
        Self {
            feed_id: [0; 32],
            price_decimals: 0,
            max_age_seconds: 0,
            max_confidence_bps: 0,
        }
    }
}

/// Oracle configuration stored by vault and asset accounts.
///
/// The serialized shape includes every supported oracle implementation from
/// the start. Switching implementations only changes `kind`, so account data
/// size remains stable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct OracleConfig {
    pub kind: OracleKind,
    pub switchboard: SwitchboardOracleConfig,
    pub pyth: PythOracleConfig,
}

impl OracleConfig {
    pub const fn kind(&self) -> OracleKind {
        self.kind
    }

    pub const fn switchboard(config: SwitchboardOracleConfig) -> Self {
        Self {
            kind: OracleKind::Switchboard,
            switchboard: config,
            pyth: PythOracleConfig {
                feed_id: [0; 32],
                price_decimals: 0,
                max_age_seconds: 0,
                max_confidence_bps: 0,
            },
        }
    }

    pub const fn pyth(config: PythOracleConfig) -> Self {
        Self {
            kind: OracleKind::Pyth,
            switchboard: SwitchboardOracleConfig {
                quote_account: [0; 32],
                queue_account: [0; 32],
                feed_id: [0; 32],
                price_decimals: 0,
                max_age_slots: 0,
            },
            pyth: config,
        }
    }

    pub const fn with_configs(
        kind: OracleKind,
        switchboard: SwitchboardOracleConfig,
        pyth: PythOracleConfig,
    ) -> Self {
        Self {
            kind,
            switchboard,
            pyth,
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
    use wincode::serialize;

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
        assert_eq!(switchboard.kind(), OracleKind::Switchboard);
        assert_eq!(pyth.kind(), OracleKind::Pyth);
    }

    #[test]
    fn with_configs_keeps_inactive_config_available() {
        let switchboard_config = SwitchboardOracleConfig::new([1; 32], [2; 32], [3; 32], 6, 100);
        let pyth_config = PythOracleConfig::new([4; 32], 8, 30, 250);

        let config = OracleConfig::with_configs(OracleKind::Pyth, switchboard_config, pyth_config);

        assert_eq!(config.kind(), OracleKind::Pyth);
        assert_eq!(config.switchboard, switchboard_config);
        assert_eq!(config.pyth, pyth_config);
    }
}
