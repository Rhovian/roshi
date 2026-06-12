mod pyth;
mod switchboard;

pub use pyth::PythOracle;
pub use roshi_interface::oracle::{
    OracleConfig, OracleKind, OraclePrice, PythOracleConfig, SwitchboardOracleConfig,
};
pub use switchboard::SwitchboardOracle;

/// Trait for oracle implementations. Implementations are expected to
/// provide parsing helper methods for extracting a price from account bytes.
/// On-chain program code should match `OracleKind` and parse the oracle
/// account(s) appropriately in instruction handlers.
pub trait Oracle {
    /// Parse a whole-token price from raw account bytes. Return None if
    /// data is unavailable, stale, or invalid.
    fn parse_price(&self, data: &[u8]) -> Option<OraclePrice>;
}
