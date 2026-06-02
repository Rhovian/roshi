use roshi_interface::instructions::InitializeProgramArgs;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

use super::{new, Result};

pub fn initialize_program(
    payer: Pubkey,
    program_config: Pubkey,
    authority: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(program_config, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        &InitializeProgramArgs {
            authority: authority.to_bytes(),
        },
    )
}
