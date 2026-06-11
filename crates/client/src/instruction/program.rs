use roshi_interface::{instructions::InitializeProgramArgs, ID};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result};

/// The transaction must also be signed by the program keypair itself
/// (`roshi_interface::ID`): initialization is bound to possession of that
/// keypair so the global config cannot be front-run after deploy.
pub fn initialize_program(
    payer: Pubkey,
    program_config: Pubkey,
    authority: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(ID, true),
            AccountMeta::new(program_config, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &InitializeProgramArgs {
            authority: authority.to_bytes(),
        },
    )
}
