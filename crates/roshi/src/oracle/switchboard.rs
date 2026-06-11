use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use switchboard_on_demand::{OracleQuote, QuoteVerifier};

use super::{Oracle, OraclePrice, SwitchboardOracleConfig};

const SWITCHBOARD_QUOTE_DISCRIMINATOR: &[u8; 8] = b"SBOracle";

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
        self.verify_account_contract(
            quote_account,
            queue_account,
            slothash_sysvar,
            instructions_sysvar,
        )?;

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

    /// Pin every depositor-supplied account before any data is read: quote and
    /// queue to the configured addresses, and the sysvars to their canonical
    /// ids. `QuoteVerifier` validates the sysvars internally today, but the
    /// account contract must not depend on dependency internals.
    fn verify_account_contract(
        &self,
        quote_account: &AccountInfo,
        queue_account: &AccountInfo,
        slothash_sysvar: &AccountInfo,
        instructions_sysvar: &AccountInfo,
    ) -> Result<(), ProgramError> {
        if quote_account.key.to_bytes() != self.config.quote_account
            || queue_account.key.to_bytes() != self.config.queue_account
            || slothash_sysvar.key != &solana_sdk_ids::sysvar::slot_hashes::ID
            || instructions_sysvar.key != &solana_sdk_ids::sysvar::instructions::ID
        {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use solana_pubkey::Pubkey;

    use super::*;

    struct AccountFixture {
        key: Pubkey,
        lamports: u64,
        data: Vec<u8>,
        owner: Pubkey,
    }

    impl AccountFixture {
        fn new(key: Pubkey) -> Self {
            Self {
                key,
                lamports: 1,
                data: Vec::new(),
                owner: Pubkey::new_unique(),
            }
        }

        fn info(&mut self) -> AccountInfo<'_> {
            AccountInfo::new(
                &self.key,
                false,
                false,
                &mut self.lamports,
                &mut self.data,
                &self.owner,
                false,
            )
        }
    }

    #[test]
    fn account_contract_pins_configured_accounts_and_canonical_sysvars() {
        let quote_key = Pubkey::new_unique();
        let queue_key = Pubkey::new_unique();
        let oracle = SwitchboardOracle::new(SwitchboardOracleConfig::new(
            quote_key.to_bytes(),
            queue_key.to_bytes(),
            [3; 32],
            6,
            100,
        ));

        let mut quote = AccountFixture::new(quote_key);
        let mut queue = AccountFixture::new(queue_key);
        let mut slothash = AccountFixture::new(solana_sdk_ids::sysvar::slot_hashes::ID);
        let mut instructions = AccountFixture::new(solana_sdk_ids::sysvar::instructions::ID);
        // The full contract holds even though no quote data is attached.
        assert_eq!(
            oracle.verify_account_contract(
                &quote.info(),
                &queue.info(),
                &slothash.info(),
                &instructions.info(),
            ),
            Ok(())
        );

        // Substituting any single account breaks the contract.
        for position in 0..4 {
            let mut impostor = AccountFixture::new(Pubkey::new_unique());
            let substitute = impostor.info();
            let quote = quote.info();
            let queue = queue.info();
            let slothash = slothash.info();
            let instructions = instructions.info();
            let pick = |slot: usize, canonical| {
                if slot == position {
                    substitute.clone()
                } else {
                    canonical
                }
            };
            assert_eq!(
                oracle.verify_account_contract(
                    &pick(0, quote),
                    &pick(1, queue),
                    &pick(2, slothash),
                    &pick(3, instructions),
                ),
                Err(ProgramError::InvalidAccountData),
                "substituting account {position} must fail",
            );
        }
    }
}
