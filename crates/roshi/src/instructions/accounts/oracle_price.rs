use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_sysvar::clock::Clock;

use crate::oracle::{OracleConfig, OracleKind, OraclePrice, PythOracle, SwitchboardOracle};

/// Read one verified oracle leg from the front of `accounts`, returning the
/// price and how many accounts the leg consumed (Pyth: 1 price update;
/// Switchboard: quote, queue, slot-hashes sysvar, instructions sysvar).
pub(crate) fn read_oracle_price<'a, 'info>(
    oracle: &OracleConfig,
    accounts: &'a [AccountInfo<'info>],
    clock: &Clock,
) -> Result<(OraclePrice, usize), ProgramError>
where
    'a: 'info,
{
    // Both holders of an OracleConfig (vault, asset) validate the kind at
    // deserialization, so an invalid kind here is corrupted state.
    let kind = oracle
        .kind()
        .map_err(|_| ProgramError::InvalidAccountData)?;

    match kind {
        OracleKind::Pyth => {
            let price_account = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
            let price = PythOracle::new(oracle.pyth)
                .read_verified_price(price_account, clock.unix_timestamp)?;
            Ok((price, 1))
        }
        OracleKind::Switchboard => {
            let quote = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
            let queue = accounts.get(1).ok_or(ProgramError::NotEnoughAccountKeys)?;
            let slothash = accounts.get(2).ok_or(ProgramError::NotEnoughAccountKeys)?;
            let ix_sysvar = accounts.get(3).ok_or(ProgramError::NotEnoughAccountKeys)?;
            let price = SwitchboardOracle::new(oracle.switchboard)
                .read_verified_price(quote, queue, slothash, ix_sysvar, clock.slot)?;
            Ok((price, 4))
        }
    }
}
