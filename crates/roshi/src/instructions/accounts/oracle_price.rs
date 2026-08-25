use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use solana_sysvar::clock::Clock;

use crate::oracle::{
    OracleConfig, OracleKind, OraclePrice, PythOracle, ScopeOracle, SwitchboardOracle,
};

/// Read one verified oracle leg from the front of `accounts`, returning its
/// price and the unconsumed accounts.
pub(crate) fn read_oracle_price<'a, 'info>(
    oracle: &OracleConfig,
    accounts: &'a [AccountInfo<'info>],
    clock: &Clock,
) -> Result<(OraclePrice, &'a [AccountInfo<'info>]), ProgramError>
where
    'a: 'info,
{
    // Both holders of an OracleConfig (vault, asset) validate the kind at
    // deserialization, so an invalid kind here is corrupted state.
    let kind = oracle
        .kind()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let (accounts, remaining) = split_oracle_accounts(oracle, accounts)?;

    let price = match kind {
        OracleKind::Pyth => {
            let price_account = &accounts[0];
            PythOracle::new(oracle.pyth_config())
                .read_verified_price(price_account, clock.unix_timestamp)?
        }
        OracleKind::Switchboard => {
            let quote = &accounts[0];
            let queue = &accounts[1];
            let slothash = &accounts[2];
            let ix_sysvar = &accounts[3];
            SwitchboardOracle::new(oracle.switchboard_config())
                .read_verified_price(quote, queue, slothash, ix_sysvar, clock.slot)?
        }
        OracleKind::Scope => {
            let prices = &accounts[0];
            let mappings = &accounts[1];
            ScopeOracle::new(oracle.scope_config()).read_verified_price(
                prices,
                mappings,
                clock.unix_timestamp,
            )?
        }
    };

    Ok((price, remaining))
}

/// Split one oracle leg from the front of `accounts`.
pub(crate) fn split_oracle_accounts<'a, 'info>(
    oracle: &OracleConfig,
    accounts: &'a [AccountInfo<'info>],
) -> Result<(&'a [AccountInfo<'info>], &'a [AccountInfo<'info>]), ProgramError> {
    let count = match oracle
        .kind()
        .map_err(|_| ProgramError::InvalidAccountData)?
    {
        OracleKind::Pyth => 1,        // PriceUpdateV2
        OracleKind::Switchboard => 4, // quote, queue, slot hashes, instructions
        OracleKind::Scope => 2,       // OraclePrices, OracleMappings
    };
    if accounts.len() < count {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    Ok(accounts.split_at(count))
}
