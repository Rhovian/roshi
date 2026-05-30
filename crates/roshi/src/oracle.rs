mod pyth;
mod switchboard;

pub use pyth::PythOracle;
pub use roshi_interface::oracle::{
    OracleConfig, OracleKind, PythOracleConfig, SwitchboardOracleConfig,
};
pub use switchboard::SwitchboardOracle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OraclePrice {
    /// Raw base-denominated price value.
    pub value: u128,
    /// Decimal scale for `value`.
    pub decimals: u8,
}

/// Trait for oracle implementations. Implementations are expected to
/// provide parsing helper methods for extracting a price from account bytes.
/// On-chain program code should match `OracleKind` and parse the oracle
/// account(s) appropriately in instruction handlers.
pub trait Oracle {
    /// Parse a base-denominated price from raw account bytes. Return None if
    /// data is unavailable, stale, or invalid.
    fn parse_price(&self, data: &[u8]) -> Option<OraclePrice>;
}
