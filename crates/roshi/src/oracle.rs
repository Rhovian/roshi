use solana_pubkey::Pubkey;

/// Discriminator for oracle implementations stored on-chain in `Asset`.
#[repr(u8)]
pub enum OracleKind {
    Switchboard = 0,
    Pyth = 1,
}

/// Trait for oracle implementations. Implementations are expected to
/// provide parsing helper methods for extracting a price from account bytes.
/// On-chain program code should match `OracleKind` and parse the oracle
/// account(s) appropriately in instruction handlers.
pub trait Oracle {
    /// Parse base units per asset atomic unit from raw account bytes. Return
    /// None if data is unavailable or stale.
    fn parse_base_units_per_asset_atom(data: &[u8]) -> Option<u128>;
}

/// Minimal Switchboard stub. Real parsing should live in the processor where
/// account infos are available; this is a lightweight placeholder to record
/// the expected shape and keep implementations testable off-chain.
pub struct SwitchboardOracle {
    pub feed: Pubkey,
}

impl SwitchboardOracle {
    pub fn new(feed: Pubkey) -> Self {
        Self { feed }
    }
}

impl Oracle for SwitchboardOracle {
    fn parse_base_units_per_asset_atom(_data: &[u8]) -> Option<u128> {
        // Placeholder: actual Switchboard parsing depends on SB account layout
        None
    }
}
