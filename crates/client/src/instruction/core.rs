use roshi_interface::{
    instructions::{serialize_instruction, InstructionArgs},
    ID,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

pub type Result<T> = core::result::Result<T, wincode::WriteError>;

pub fn new<T>(accounts: Vec<AccountMeta>, args: &T) -> Result<Instruction>
where
    T: InstructionArgs,
{
    new_with_program_id(ID, accounts, args)
}

pub fn new_with_program_id<T>(
    program_id: Pubkey,
    accounts: Vec<AccountMeta>,
    args: &T,
) -> Result<Instruction>
where
    T: InstructionArgs,
{
    Ok(Instruction {
        program_id,
        accounts,
        data: serialize_instruction(args)?,
    })
}
