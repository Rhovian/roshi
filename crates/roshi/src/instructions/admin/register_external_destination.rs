use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{
    accounts::RegisterExternalDestinationContext, RegisterExternalDestinationArgs,
};

/// Implements [`crate::instructions::RoshiInstruction::RegisterExternalDestination`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Vault admin (funds the registration account).
/// 1. `[]` Vault.
/// 2. `[]` Destination base token account being authorized.
/// 3. `[writable]` ExternalDestination PDA derived from `(vault, destination)`.
/// 4. `[]` System program.
///
/// Verifies the vault admin and records the destination, after which
/// `invest_external` may move custody to it. The admin authorizes venues;
/// the strategist only moves funds between custody and authorized venues.
pub fn try_register_external_destination(
    accounts: &[AccountInfo],
    RegisterExternalDestinationArgs: RegisterExternalDestinationArgs,
) -> ProgramResult {
    RegisterExternalDestinationContext::load(accounts)?.create_and_store()
}
