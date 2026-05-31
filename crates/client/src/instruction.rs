use roshi_interface::{
    instructions::{
        IndexedActionArgs, InitializeVaultArgs, RoshiInstruction, SetNavAuthorityArgs,
        SetStrategistArgs, SetVaultAccessArgs, SetWithdrawalAuthorityArgs,
    },
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

pub fn initialize_vault(
    program_authority: Pubkey,
    program_config: Pubkey,
    payer: Pubkey,
    vault: Pubkey,
    args: InitializeVaultArgs,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(program_authority, true),
            AccountMeta::new_readonly(program_config, false),
            AccountMeta::new(payer, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        RoshiInstruction::InitializeVault { args },
    )
}

pub fn transfer_program_authority(
    authority: Pubkey,
    program_config: Pubkey,
    new_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(program_config, false),
        ],
        RoshiInstruction::TransferProgramAuthority {
            new_authority: new_authority.to_bytes(),
        },
    )
}

pub fn transfer_vault_authority(
    authority: Pubkey,
    vault: Pubkey,
    new_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(vault, false),
        ],
        RoshiInstruction::TransferVaultAuthority {
            new_authority: new_authority.to_bytes(),
        },
    )
}

fn vault_admin_accounts(admin: Pubkey, vault: Pubkey) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new_readonly(admin, true),
        AccountMeta::new(vault, false),
    ]
}

pub fn set_strategist(admin: Pubkey, vault: Pubkey, strategist: Pubkey) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        RoshiInstruction::SetStrategist {
            args: SetStrategistArgs {
                strategist: strategist.to_bytes(),
            },
        },
    )
}

pub fn set_nav_authority(
    admin: Pubkey,
    vault: Pubkey,
    nav_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        RoshiInstruction::SetNavAuthority {
            args: SetNavAuthorityArgs {
                nav_authority: nav_authority.to_bytes(),
            },
        },
    )
}

pub fn set_withdrawal_authority(
    admin: Pubkey,
    vault: Pubkey,
    withdrawal_authority: Pubkey,
) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        RoshiInstruction::SetWithdrawalAuthority {
            args: SetWithdrawalAuthorityArgs {
                withdrawal_authority: withdrawal_authority.to_bytes(),
            },
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

pub fn deposit(
    depositor: Pubkey,
    vault: Pubkey,
    user_source_token_account: Pubkey,
    vault_custody_token_account: Pubkey,
    user_share_account: Pubkey,
    asset_mint: Pubkey,
    amount: u64,
    min_shares_out: u64,
    access_proof: Vec<[u8; 32]>,
    additional_accounts: Vec<AccountMeta>,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new_readonly(depositor, true),
        AccountMeta::new(vault, false),
        AccountMeta::new(user_source_token_account, false),
        AccountMeta::new(vault_custody_token_account, false),
        AccountMeta::new(user_share_account, false),
    ];
    accounts.extend(additional_accounts);

    new(
        accounts,
        RoshiInstruction::Deposit {
            asset_mint: asset_mint.to_bytes(),
            amount,
            min_shares_out,
            access_proof,
        },
    )
}

pub fn set_vault_access(
    admin: Pubkey,
    vault: Pubkey,
    private: bool,
    access_merkle_root: [u8; 32],
) -> Result<Instruction> {
    new(
        vault_admin_accounts(admin, vault),
        RoshiInstruction::SetVaultAccess {
            args: SetVaultAccessArgs {
                private,
                access_merkle_root,
            },
        },
    )
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
    fn builds_initialize_vault_instruction() {
        let program_authority = Pubkey::new_unique();
        let program_config = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let share_mint = Pubkey::new_unique();
        let args = InitializeVaultArgs {
            tag: [1; 32],
            tag_len: 4,
            admin: Pubkey::new_unique().to_bytes(),
            strategist: Pubkey::new_unique().to_bytes(),
            nav_authority: Pubkey::new_unique().to_bytes(),
            withdrawal_authority: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            share_mint: share_mint.to_bytes(),
            base_decimals: 6,
            base_oracle: roshi_interface::oracle::OracleConfig::default(),
            deposit_sub_account: 0,
            withdraw_sub_account: 1,
            fee_collector: Pubkey::new_unique().to_bytes(),
            performance_fee_bps: 100,
            withdrawal_buffer_bps: 250,
            max_change_bps: 500,
            min_update_interval: 60,
            private: true,
            access_merkle_root: [2; 32],
        };

        let ix = initialize_vault(program_authority, program_config, payer, vault, args).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 5);
        assert_eq!(
            ix.accounts[0],
            AccountMeta::new_readonly(program_authority, true)
        );
        assert_eq!(
            ix.accounts[1],
            AccountMeta::new_readonly(program_config, false)
        );
        assert_eq!(ix.accounts[2], AccountMeta::new(payer, true));
        assert_eq!(ix.accounts[3], AccountMeta::new(vault, false));
        assert_eq!(
            ix.accounts[4],
            AccountMeta::new_readonly(system_program::ID, false)
        );

        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::InitializeVault { args } => {
                assert_eq!(args.tag, [1; 32]);
                assert_eq!(args.tag_len, 4);
                assert_eq!(args.base_mint, base_mint.to_bytes());
                assert_eq!(args.share_mint, share_mint.to_bytes());
                assert!(args.private);
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

    #[test]
    fn builds_transfer_program_authority_instruction() {
        let authority = Pubkey::new_unique();
        let program_config = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();

        let ix = transfer_program_authority(authority, program_config, new_authority).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 2);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(authority, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(program_config, false));

        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::TransferProgramAuthority {
                new_authority: decoded,
            } => {
                assert_eq!(decoded, new_authority.to_bytes());
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn builds_transfer_vault_authority_instruction() {
        let authority = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();

        let ix = transfer_vault_authority(authority, vault, new_authority).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 2);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(authority, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));

        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::TransferVaultAuthority {
                new_authority: decoded,
            } => {
                assert_eq!(decoded, new_authority.to_bytes());
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn builds_vault_role_setter_instructions() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let strategist = Pubkey::new_unique();
        let nav_authority = Pubkey::new_unique();
        let withdrawal_authority = Pubkey::new_unique();

        let ix = set_strategist(admin, vault, strategist).unwrap();
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::SetStrategist { args } => {
                assert_eq!(args.strategist, strategist.to_bytes());
            }
            _ => panic!("unexpected instruction"),
        }

        let ix = set_nav_authority(admin, vault, nav_authority).unwrap();
        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::SetNavAuthority { args } => {
                assert_eq!(args.nav_authority, nav_authority.to_bytes());
            }
            _ => panic!("unexpected instruction"),
        }

        let ix = set_withdrawal_authority(admin, vault, withdrawal_authority).unwrap();
        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::SetWithdrawalAuthority { args } => {
                assert_eq!(args.withdrawal_authority, withdrawal_authority.to_bytes());
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn builds_deposit_instruction_with_access_proof() {
        let depositor = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let custody = Pubkey::new_unique();
        let shares = Pubkey::new_unique();
        let asset_mint = Pubkey::new_unique();
        let asset_pda = Pubkey::new_unique();
        let proof = vec![[1; 32], [2; 32]];

        let ix = deposit(
            depositor,
            vault,
            source,
            custody,
            shares,
            asset_mint,
            123,
            456,
            proof.clone(),
            vec![AccountMeta::new_readonly(asset_pda, false)],
        )
        .unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 6);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(depositor, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(source, false));
        assert_eq!(ix.accounts[3], AccountMeta::new(custody, false));
        assert_eq!(ix.accounts[4], AccountMeta::new(shares, false));
        assert_eq!(ix.accounts[5], AccountMeta::new_readonly(asset_pda, false));

        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::Deposit {
                asset_mint: decoded_asset_mint,
                amount,
                min_shares_out,
                access_proof,
            } => {
                assert_eq!(decoded_asset_mint, asset_mint.to_bytes());
                assert_eq!(amount, 123);
                assert_eq!(min_shares_out, 456);
                assert_eq!(access_proof, proof);
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn builds_set_vault_access_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let root = [9; 32];

        let ix = set_vault_access(admin, vault, true, root).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 2);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));

        match wincode::deserialize(&ix.data).unwrap() {
            RoshiInstruction::SetVaultAccess { args } => {
                assert!(args.private);
                assert_eq!(args.access_merkle_root, root);
            }
            _ => panic!("unexpected instruction"),
        }
    }
}
