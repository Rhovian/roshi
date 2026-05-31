use roshi_interface::{
    instructions::{
        serialize_instruction, DepositArgs, InitializeProgramArgs, InitializeVaultArgs,
        InstructionArgs, ManageArgs, ManageBatchArgs, SetNavAuthorityArgs, SetStrategistArgs,
        SetVaultAccessArgs, SetWithdrawalAuthorityArgs, TransferProgramAuthorityArgs,
        TransferVaultAuthorityArgs,
    },
    ID,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_system_interface::program as system_program;

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
        &args,
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
        &TransferProgramAuthorityArgs {
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
        &TransferVaultAuthorityArgs {
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
        &SetStrategistArgs {
            strategist: strategist.to_bytes(),
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
        &SetNavAuthorityArgs {
            nav_authority: nav_authority.to_bytes(),
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
        &SetWithdrawalAuthorityArgs {
            withdrawal_authority: withdrawal_authority.to_bytes(),
        },
    )
}

pub fn manage(
    strategist: Pubkey,
    vault: Pubkey,
    sub_account_pda: Pubkey,
    action: Pubkey,
    cpi_accounts: Vec<AccountMeta>,
    args: ManageArgs,
) -> Result<Instruction> {
    let mut accounts = vec![
        AccountMeta::new_readonly(strategist, true),
        AccountMeta::new_readonly(vault, false),
        AccountMeta::new(sub_account_pda, false),
        AccountMeta::new_readonly(action, false),
    ];
    accounts.extend(cpi_accounts);

    new(accounts, &args)
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
    actions: Vec<ManageArgs>,
) -> Result<Instruction> {
    let mut accounts = Vec::with_capacity(2 + action_accounts.len() * 2 + cpi_accounts.len());
    accounts.push(AccountMeta::new_readonly(strategist, true));
    accounts.push(AccountMeta::new_readonly(vault, false));

    for action_accounts in action_accounts {
        accounts.push(AccountMeta::new(action_accounts.sub_account_pda, false));
        accounts.push(AccountMeta::new_readonly(action_accounts.action, false));
    }

    accounts.extend(cpi_accounts);

    new(accounts, &ManageBatchArgs { actions })
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
        &DepositArgs {
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
        &SetVaultAccessArgs {
            private,
            access_merkle_root,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincode::{config::DefaultConfig, SchemaRead};

    fn decode_args<'a, T>(data: &'a [u8]) -> T
    where
        T: InstructionArgs + SchemaRead<'a, DefaultConfig, Dst = T>,
    {
        assert_eq!(data[0], u8::from(T::TAG));
        wincode::deserialize_exact(&data[1..]).unwrap()
    }

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

        let args: InitializeProgramArgs = decode_args(&ix.data);
        assert_eq!(args.authority, authority.to_bytes());
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

        let args: InitializeVaultArgs = decode_args(&ix.data);
        assert_eq!(args.tag, [1; 32]);
        assert_eq!(args.tag_len, 4);
        assert_eq!(args.base_mint, base_mint.to_bytes());
        assert_eq!(args.share_mint, share_mint.to_bytes());
        assert!(args.private);
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
            ManageArgs {
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

        let args: ManageArgs = decode_args(&ix.data);
        assert_eq!(args.sub_account, 7);
        assert_eq!(args.program_id, cpi_program.to_bytes());
        assert_eq!(args.accounts_start, 0);
        assert_eq!(args.accounts_len, 1);
        assert_eq!(args.ix_data, ix_data);
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

        let args: TransferProgramAuthorityArgs = decode_args(&ix.data);
        assert_eq!(args.new_authority, new_authority.to_bytes());
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

        let args: TransferVaultAuthorityArgs = decode_args(&ix.data);
        assert_eq!(args.new_authority, new_authority.to_bytes());
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
        let args: SetStrategistArgs = decode_args(&ix.data);
        assert_eq!(args.strategist, strategist.to_bytes());

        let ix = set_nav_authority(admin, vault, nav_authority).unwrap();
        let args: SetNavAuthorityArgs = decode_args(&ix.data);
        assert_eq!(args.nav_authority, nav_authority.to_bytes());

        let ix = set_withdrawal_authority(admin, vault, withdrawal_authority).unwrap();
        let args: SetWithdrawalAuthorityArgs = decode_args(&ix.data);
        assert_eq!(args.withdrawal_authority, withdrawal_authority.to_bytes());
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

        let args: DepositArgs = decode_args(&ix.data);
        assert_eq!(args.asset_mint, asset_mint.to_bytes());
        assert_eq!(args.amount, 123);
        assert_eq!(args.min_shares_out, 456);
        assert_eq!(args.access_proof, proof);
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

        let args: SetVaultAccessArgs = decode_args(&ix.data);
        assert!(args.private);
        assert_eq!(args.access_merkle_root, root);
    }
}
