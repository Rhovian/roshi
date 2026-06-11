use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;
use solana_pubkey::Pubkey;

use crate::{
    instructions::accounts::InitializeProgramContext, instructions::InitializeProgramArgs,
};

/// Implements [`crate::instructions::RoshiInstruction::InitializeProgram`].
///
/// # Accounts
///
/// 0. `[signer, writable]` Payer that funds the program config account.
/// 1. `[signer]` The program account itself (`crate::ID`).
/// 2. `[writable]` Program config PDA derived from `ProgramConfig::SEED`.
/// 3. `[]` System program.
///
/// # Implementation
///
/// Validates the payer, requires the program's own keypair as a co-signer (the
/// config PDA is a global singleton, so this prevents an observer from
/// front-running initialization and seizing the program authority), rejects an
/// already initialized config account, verifies the system program, checks the
/// config PDA seeds, creates the config account with rent-exempt lamports, and
/// stores the configured program authority.
pub fn try_initialize_program(
    accounts: &[AccountInfo],
    args: InitializeProgramArgs,
) -> ProgramResult {
    let accounts = InitializeProgramContext::load(accounts)?;

    accounts.create_config_account()?;
    accounts.store_config(Pubkey::from(args.authority))
}
