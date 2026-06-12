use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

use crate::instructions::{
    accounts::close_external_destination_as_admin, RevokeExternalDestinationArgs,
};

/// Implements [`crate::instructions::RoshiInstruction::RevokeExternalDestination`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Vault admin (receives the reclaimed rent).
/// 1. `[]` Vault.
/// 2. `[writable]` ExternalDestination PDA being revoked.
///
/// Verifies the vault admin and closes the registration; subsequent
/// `invest_external` calls to that destination are rejected. Funds already
/// at the destination are unaffected — `return_external` stays open.
pub fn try_revoke_external_destination(
    accounts: &[AccountInfo],
    RevokeExternalDestinationArgs: RevokeExternalDestinationArgs,
) -> ProgramResult {
    close_external_destination_as_admin(accounts)
}
