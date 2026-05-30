use roshi_interface::{
    instructions::{IndexedActionArgs, RoshiInstruction},
    ID,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

pub type Result<T> = core::result::Result<T, wincode::WriteError>;

pub fn new(accounts: Vec<AccountMeta>, instruction: RoshiInstruction) -> Result<Instruction> {
    new_with_program_id(ID, accounts, instruction)
}

pub fn new_with_program_id(
    program_id: Pubkey,
    accounts: Vec<AccountMeta>,
    instruction: RoshiInstruction,
) -> Result<Instruction> {
    Ok(Instruction {
        program_id,
        accounts,
        data: wincode::serialize(&instruction)?,
    })
}

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
        RoshiInstruction::InitializeProgram {
            authority: authority.to_bytes(),
        },
    )
}

pub fn manage(
    strategist: Pubkey,
    vault: Pubkey,
    sub_account_pda: Pubkey,
    action: Pubkey,
    cpi_accounts: Vec<AccountMeta>,
    args: IndexedActionArgs,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new_readonly(strategist, true),
        AccountMeta::new_readonly(vault, false),
        AccountMeta::new(sub_account_pda, false),
        AccountMeta::new_readonly(action, false),
    ];
    accounts.extend(cpi_accounts);

    new(
        accounts,
        RoshiInstruction::Manage {
            sub_account: args.sub_account,
            program_id: args.program_id,
            accounts_start: args.accounts_start,
            accounts_len: args.accounts_len,
            ix_data: args.ix_data,
        },
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManageBatchActionAccounts {
    pub sub_account_pda: Pubkey,
    pub action: Pubkey,
}

pub fn manage_batch(
    strategist: Pubkey,
    vault: Pubkey,
    action_accounts: Vec<ManageBatchActionAccounts>,
    cpi_accounts: Vec<AccountMeta>,
    actions: Vec<IndexedActionArgs>,
) -> Result<Instruction> {
    let mut accounts = Vec::with_capacity(2 + action_accounts.len() * 2 + cpi_accounts.len());
    accounts.push(AccountMeta::new_readonly(strategist, true));
    accounts.push(AccountMeta::new_readonly(vault, false));

    for action_accounts in action_accounts {
        accounts.push(AccountMeta::new(action_accounts.sub_account_pda, false));
        accounts.push(AccountMeta::new_readonly(action_accounts.action, false));
    }

    accounts.extend(cpi_accounts);

    new(accounts, RoshiInstruction::ManageBatch { actions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_initialize_program_instruction() {
        let payer = Pubkey::new_unique();
        let program_config = Pubkey::new_unique();
        let authority = Pubkey::new_unique();

        let ix = initialize_program(payer, program_config, authority).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.accounts[0], AccountMeta::new(payer, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(program_config, false));
        assert_eq!(
            ix.accounts[2],
            AccountMeta::new_readonly(system_program::ID, false)
        );

        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::InitializeProgram { authority: decoded } => {
                assert_eq!(decoded, authority.to_bytes());
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn builds_manage_instruction() {
        let strategist = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let sub_account_pda = Pubkey::new_unique();
        let action = Pubkey::new_unique();
        let cpi_account = Pubkey::new_unique();
        let cpi_program = Pubkey::new_unique();
        let ix_data = vec![1, 2, 3];

        let ix = manage(
            strategist,
            vault,
            sub_account_pda,
            action,
            vec![
                AccountMeta::new(cpi_account, false),
                AccountMeta::new_readonly(cpi_program, false),
            ],
            IndexedActionArgs {
                sub_account: 7,
                program_id: cpi_program.to_bytes(),
                accounts_start: 0,
                accounts_len: 1,
                ix_data: ix_data.clone(),
            },
        )
        .unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 6);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(strategist, true));
        assert_eq!(ix.accounts[1], AccountMeta::new_readonly(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(sub_account_pda, false));
        assert_eq!(ix.accounts[3], AccountMeta::new_readonly(action, false));

        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::Manage {
                sub_account,
                program_id,
                accounts_start,
                accounts_len,
                ix_data: decoded_ix_data,
            } => {
                assert_eq!(sub_account, 7);
                assert_eq!(program_id, cpi_program.to_bytes());
                assert_eq!(accounts_start, 0);
                assert_eq!(accounts_len, 1);
                assert_eq!(decoded_ix_data, ix_data);
            }
            _ => panic!("unexpected instruction"),
        }
    }
}
