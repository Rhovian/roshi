use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;

use super::{Oracle, OraclePrice, PythOracleConfig};
use roshi_interface::math::BPS_DENOMINATOR;

const PRICE_UPDATE_V2_DISCRIMINATOR: &[u8; 8] = &[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd];
const SOLANA_RECEIVER_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

/// Pyth pull-oracle reader.
///
/// The config stores the feed id, desired output scale, max update age,
/// optional confidence-width guardrail, and an optional pinned price update
/// account. Unpinned, callers may pass either a fixed price feed account or an
/// ephemeral price update account, as long as it is owned by the Pyth Solana
/// Receiver program and contains the configured feed id; pinned, only that
/// exact account is accepted.
pub struct PythOracle {
    pub config: PythOracleConfig,
}

impl PythOracle {
    pub const fn new(config: PythOracleConfig) -> Self {
        Self { config }
    }

    /// Parse a Pyth price update account without freshness or verification
    /// checks. This is useful for tests and off-chain inspection.
    pub fn parse_unverified_price(&self, data: &[u8]) -> Option<OraclePrice> {
        let price_update = PriceUpdate::parse(data)?;
        if price_update.price.feed_id != self.config.feed_id {
            return None;
        }

        self.price_from_pyth(&price_update.price)
    }

    /// Read a fully verified Pyth price update account.
    ///
    /// This checks the configured account pin (when set), Pyth receiver
    /// ownership, parses the `PriceUpdateV2` account, validates the configured
    /// feed id and max age, and returns a positive fixed-point price at
    /// `price_decimals`.
    pub fn read_verified_price(
        &self,
        price_update_account: &AccountInfo,
        unix_timestamp: i64,
    ) -> Result<OraclePrice, ProgramError> {
        if let Some(pinned) = self.config.pinned_price_update_account() {
            if price_update_account.key.to_bytes() != pinned {
                return Err(ProgramError::InvalidAccountData);
            }
        }

        if price_update_account.owner != &SOLANA_RECEIVER_PROGRAM_ID {
            return Err(ProgramError::IllegalOwner);
        }

        let data = price_update_account.data.borrow();
        let price_update = PriceUpdate::parse(&data).ok_or(ProgramError::InvalidAccountData)?;

        self.read_verified_update(&price_update, unix_timestamp)
    }

    fn read_verified_update(
        &self,
        price_update: &PriceUpdate,
        unix_timestamp: i64,
    ) -> Result<OraclePrice, ProgramError> {
        if self.config.max_age_seconds > i64::MAX as u64 {
            return Err(ProgramError::InvalidAccountData);
        }

        if price_update.verification_level != VerificationLevel::Full
            || price_update.price.feed_id != self.config.feed_id
            || price_update
                .price
                .publish_time
                .saturating_add(self.config.max_age_seconds as i64)
                < unix_timestamp
        {
            return Err(ProgramError::InvalidAccountData);
        }

        self.price_from_pyth(&price_update.price)
            .ok_or(ProgramError::InvalidAccountData)
    }

    fn price_from_pyth(&self, price: &Price) -> Option<OraclePrice> {
        if price.price <= 0
            || !confidence_within_bounds(
                u128::try_from(price.price).ok()?,
                price.conf,
                self.config.max_confidence_bps,
            )
        {
            return None;
        }

        let value = scale_price(
            u128::try_from(price.price).ok()?,
            price.exponent,
            self.config.price_decimals,
        )?;
        if value == 0 {
            return None;
        }

        Some(OraclePrice {
            value,
            decimals: self.config.price_decimals,
        })
    }
}

impl Oracle for PythOracle {
    fn parse_price(&self, data: &[u8]) -> Option<OraclePrice> {
        self.parse_unverified_price(data)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PriceUpdate {
    verification_level: VerificationLevel,
    price: Price,
    #[allow(dead_code)]
    posted_slot: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VerificationLevel {
    Partial { num_signatures: u8 },
    Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Price {
    feed_id: [u8; 32],
    price: i64,
    conf: u64,
    exponent: i32,
    publish_time: i64,
}

impl PriceUpdate {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < PRICE_UPDATE_V2_DISCRIMINATOR.len()
            || &data[..8] != PRICE_UPDATE_V2_DISCRIMINATOR
        {
            return None;
        }

        let mut offset = 8;
        let _write_authority = read_array::<32>(data, &mut offset)?;
        let verification_level = VerificationLevel::parse(data, &mut offset)?;
        let price = Price::parse(data, &mut offset)?;
        let posted_slot = read_u64(data, &mut offset)?;

        Some(Self {
            verification_level,
            price,
            posted_slot,
        })
    }
}

impl VerificationLevel {
    fn parse(data: &[u8], offset: &mut usize) -> Option<Self> {
        match read_u8(data, offset)? {
            0 => Some(Self::Partial {
                num_signatures: read_u8(data, offset)?,
            }),
            1 => Some(Self::Full),
            _ => None,
        }
    }
}

impl Price {
    fn parse(data: &[u8], offset: &mut usize) -> Option<Self> {
        Some(Self {
            feed_id: read_array::<32>(data, offset)?,
            price: read_i64(data, offset)?,
            conf: read_u64(data, offset)?,
            exponent: read_i32(data, offset)?,
            publish_time: read_i64(data, offset)?,
        })
        .and_then(|price| {
            let _prev_publish_time = read_i64(data, offset)?;
            let _ema_price = read_i64(data, offset)?;
            let _ema_conf = read_u64(data, offset)?;
            Some(price)
        })
    }
}

fn read_array<const N: usize>(data: &[u8], offset: &mut usize) -> Option<[u8; N]> {
    let bytes = read_bytes(data, offset, N)?;
    bytes.try_into().ok()
}

fn read_u8(data: &[u8], offset: &mut usize) -> Option<u8> {
    Some(read_bytes(data, offset, 1)?[0])
}

fn read_i32(data: &[u8], offset: &mut usize) -> Option<i32> {
    Some(i32::from_le_bytes(read_array(data, offset)?))
}

fn read_i64(data: &[u8], offset: &mut usize) -> Option<i64> {
    Some(i64::from_le_bytes(read_array(data, offset)?))
}

fn read_u64(data: &[u8], offset: &mut usize) -> Option<u64> {
    Some(u64::from_le_bytes(read_array(data, offset)?))
}

fn read_bytes<'a>(data: &'a [u8], offset: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(len)?;
    let bytes = data.get(*offset..end)?;
    *offset = end;
    Some(bytes)
}

fn confidence_within_bounds(price: u128, confidence: u64, max_confidence_bps: u16) -> bool {
    if max_confidence_bps == 0 {
        return true;
    }

    u128::from(confidence)
        .checked_mul(u128::from(BPS_DENOMINATOR))
        .zip(price.checked_mul(u128::from(max_confidence_bps)))
        .is_some_and(|(confidence_bps, max_confidence)| confidence_bps <= max_confidence)
}

fn scale_price(price: u128, exponent: i32, decimals: u8) -> Option<u128> {
    let scale = i32::from(decimals).checked_add(exponent)?;
    if scale >= 0 {
        price.checked_mul(10u128.checked_pow(scale as u32)?)
    } else {
        let divisor = 10u128.checked_pow(scale.checked_abs()? as u32)?;
        Some(price / divisor)
    }
}

#[cfg(test)]
mod tests {
    use solana_pubkey::Pubkey;

    use super::*;

    const FEED_ID: [u8; 32] = [7; 32];

    fn pyth_oracle(max_confidence_bps: u16) -> PythOracle {
        PythOracle::new(PythOracleConfig::new(FEED_ID, 8, 30, max_confidence_bps))
    }

    fn price_update(
        feed_id: [u8; 32],
        verification_level: VerificationLevel,
        price: i64,
        conf: u64,
        exponent: i32,
        publish_time: i64,
    ) -> PriceUpdate {
        PriceUpdate::parse(&serialize_price_update(
            feed_id,
            verification_level,
            price,
            conf,
            exponent,
            publish_time,
        ))
        .unwrap()
    }

    fn serialize_price_update(
        feed_id: [u8; 32],
        verification_level: VerificationLevel,
        price: i64,
        conf: u64,
        exponent: i32,
        publish_time: i64,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(PRICE_UPDATE_V2_DISCRIMINATOR);
        data.extend_from_slice(&[42; 32]);
        match verification_level {
            VerificationLevel::Partial { num_signatures } => {
                data.push(0);
                data.push(num_signatures);
            }
            VerificationLevel::Full => data.push(1),
        }
        data.extend_from_slice(&feed_id);
        data.extend_from_slice(&price.to_le_bytes());
        data.extend_from_slice(&conf.to_le_bytes());
        data.extend_from_slice(&exponent.to_le_bytes());
        data.extend_from_slice(&publish_time.to_le_bytes());
        data.extend_from_slice(&publish_time.saturating_sub(1).to_le_bytes());
        data.extend_from_slice(&0i64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data
    }

    #[test]
    fn parse_unverified_price_scales_exponent() {
        let oracle = pyth_oracle(0);
        let data = serialize_price_update(
            FEED_ID,
            VerificationLevel::Partial { num_signatures: 0 },
            123_456_789,
            1_000,
            -8,
            1_000,
        );

        let price = oracle.parse_price(&data).unwrap();

        assert_eq!(
            price,
            OraclePrice {
                value: 123_456_789,
                decimals: 8,
            }
        );
    }

    #[test]
    fn read_verified_update_enforces_verification_and_age() {
        let oracle = pyth_oracle(0);
        let partial = price_update(
            FEED_ID,
            VerificationLevel::Partial { num_signatures: 5 },
            123_456_789,
            1_000,
            -8,
            1_000,
        );
        let stale = price_update(
            FEED_ID,
            VerificationLevel::Full,
            123_456_789,
            1_000,
            -8,
            1_000,
        );

        assert!(oracle.read_verified_update(&partial, 1_005).is_err());
        assert!(oracle.read_verified_update(&stale, 1_031).is_err());
        assert!(oracle.read_verified_update(&stale, 1_030).is_ok());
    }

    #[test]
    fn read_verified_update_rejects_mismatch_and_wide_confidence() {
        let oracle = pyth_oracle(100);
        let mismatched = price_update([9; 32], VerificationLevel::Full, 10_000, 1, -2, 1_000);
        let wide_confidence =
            price_update(FEED_ID, VerificationLevel::Full, 10_000, 101, -2, 1_000);
        let acceptable = price_update(FEED_ID, VerificationLevel::Full, 10_000, 100, -2, 1_000);

        assert!(oracle.read_verified_update(&mismatched, 1_000).is_err());
        assert!(oracle
            .read_verified_update(&wide_confidence, 1_000)
            .is_err());
        assert!(oracle.read_verified_update(&acceptable, 1_000).is_ok());
    }

    #[test]
    fn read_verified_price_checks_account_owner() {
        let oracle = pyth_oracle(0);
        let mut data = serialize_price_update(
            FEED_ID,
            VerificationLevel::Full,
            123_456_789,
            1_000,
            -8,
            1_000,
        );

        let key = Pubkey::new_unique();
        let owner = SOLANA_RECEIVER_PROGRAM_ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);
        assert!(oracle.read_verified_price(&account, 1_000).is_ok());

        let wrong_owner = Pubkey::new_unique();
        let mut lamports = 1;
        let mut data = serialize_price_update(
            FEED_ID,
            VerificationLevel::Full,
            123_456_789,
            1_000,
            -8,
            1_000,
        );
        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &wrong_owner,
            false,
        );
        assert_eq!(
            oracle.read_verified_price(&account, 1_000),
            Err(ProgramError::IllegalOwner)
        );
    }

    #[test]
    fn read_verified_price_enforces_account_pin() {
        let pinned_key = Pubkey::new_unique();
        let oracle = PythOracle::new(
            PythOracleConfig::new(FEED_ID, 8, 30, 0)
                .pin_price_update_account(pinned_key.to_bytes()),
        );
        let owner = SOLANA_RECEIVER_PROGRAM_ID;

        let mut data = serialize_price_update(
            FEED_ID,
            VerificationLevel::Full,
            123_456_789,
            1_000,
            -8,
            1_000,
        );
        let mut lamports = 1;
        let account = AccountInfo::new(
            &pinned_key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
        );
        assert!(oracle.read_verified_price(&account, 1_000).is_ok());

        // The same verified update under any other address must be rejected.
        let other_key = Pubkey::new_unique();
        let mut data = serialize_price_update(
            FEED_ID,
            VerificationLevel::Full,
            123_456_789,
            1_000,
            -8,
            1_000,
        );
        let mut lamports = 1;
        let account = AccountInfo::new(
            &other_key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
        );
        assert_eq!(
            oracle.read_verified_price(&account, 1_000),
            Err(ProgramError::InvalidAccountData)
        );
    }

    #[test]
    fn scale_price_handles_coarser_and_finer_decimals() {
        assert_eq!(scale_price(123_456_789, -8, 8), Some(123_456_789));
        assert_eq!(scale_price(123_456_789, -8, 10), Some(12_345_678_900));
        assert_eq!(scale_price(123_456_789, -8, 6), Some(1_234_567));
    }
}
