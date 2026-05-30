use wincode::{SchemaRead, SchemaWrite};

/// Discriminator for oracle implementations.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OracleKind {
    Switchboard = 0,
    Pyth = 1,
    Doppler = 2,
}

impl OracleKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(kind: u8) -> Option<Self> {
        match kind {
            0 => Some(Self::Switchboard),
            1 => Some(Self::Pyth),
            2 => Some(Self::Doppler),
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
