use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use switchboard_on_demand::{OracleQuote, QuoteVerifier};

pub use roshi_interface::oracle::{OracleKind, SwitchboardOracleConfig};

const SWITCHBOARD_QUOTE_DISCRIMINATOR: &[u8; 8] = b"SBOracle";

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

/// Switchboard On-Demand oracle reader.
///
/// The config stores the quote account, queue account, and 32-byte feed id
/// expected inside the quote.
pub struct SwitchboardOracle {
    pub config: SwitchboardOracleConfig,
}

impl SwitchboardOracle {
    pub fn new(config: SwitchboardOracleConfig) -> Self {
        Self { config }
    }
}

impl Oracle for SwitchboardOracle {
    fn parse_price(&self, data: &[u8]) -> Option<OraclePrice> {
        self.parse_unverified_price(data)
    }
}

impl SwitchboardOracle {
    /// Parse a Switchboard quote account without cryptographic verification.
    ///
    /// This is useful for tests and off-chain inspection. State-changing
    /// instruction handlers should use [`Self::read_verified_price`].
    pub fn parse_unverified_price(&self, data: &[u8]) -> Option<OraclePrice> {
        if data.len() < 40 || &data[..8] != SWITCHBOARD_QUOTE_DISCRIMINATOR {
            return None;
        }

        let quote = QuoteVerifier::new()
            .parse_unverified_delimited(&data[40..])
            .ok()?;

        self.price_from_quote(&quote)
    }

    /// Verify a Switchboard quote account and read the configured feed value.
    ///
    /// The verification path follows Switchboard's advanced price-feed pattern:
    /// queue account, slot hashes sysvar, instructions sysvar, current clock
    /// slot, and max-age are all provided to `QuoteVerifier` before the quote
    /// account is read.
    pub fn read_verified_price<'info>(
        &self,
        quote_account: &'info AccountInfo<'info>,
        queue_account: &'info AccountInfo<'info>,
        slothash_sysvar: &'info AccountInfo<'info>,
        instructions_sysvar: &'info AccountInfo<'info>,
        clock_slot: u64,
    ) -> Result<OraclePrice, ProgramError> {
        if quote_account.key.to_bytes() != self.config.quote_account
            || queue_account.key.to_bytes() != self.config.queue_account
        {
            return Err(ProgramError::InvalidAccountData);
        }

        let queue_key = queue_account.key.to_bytes();
        let quote = QuoteVerifier::new()
            .queue(queue_account)
            .slothash_sysvar(slothash_sysvar)
            .ix_sysvar(instructions_sysvar)
            .clock_slot(clock_slot)
            .max_age(self.config.max_age_slots)
            .verify_account(&queue_key, quote_account)
            .map_err(|_| ProgramError::InvalidAccountData)?;

        self.price_from_quote(&quote)
            .ok_or(ProgramError::InvalidAccountData)
    }

    fn price_from_quote(&self, quote: &OracleQuote) -> Option<OraclePrice> {
        let feed = quote.feed(&self.config.feed_id).ok()?;
        let raw_value = feed.feed_value();
        if raw_value <= 0 {
            return None;
        }

        Some(OraclePrice {
            value: u128::try_from(raw_value).ok()?,
            decimals: self.config.price_decimals,
        })
    }
}

/// Minimal Pyth stub. Real parsing should validate the Pyth account layout,
/// freshness, confidence bounds, and exponent scaling before returning a
/// base-denominated value.
pub struct PythOracle;

impl PythOracle {
    pub const fn new() -> Self {
        Self
    }
}

impl Oracle for PythOracle {
    fn parse_price(&self, _data: &[u8]) -> Option<OraclePrice> {
        // Placeholder: actual Pyth parsing depends on Pyth account layout
        None
    }
}

/// Minimal Doppler stub. Real parsing should validate the Doppler account
/// layout, freshness, and scaling before returning a base-denominated value.
pub struct DopplerOracle;

impl DopplerOracle {
    pub const fn new() -> Self {
        Self
    }
}

impl Oracle for DopplerOracle {
    fn parse_price(&self, _data: &[u8]) -> Option<OraclePrice> {
        // Placeholder: actual Doppler parsing depends on Doppler account layout
        None
    }
}
