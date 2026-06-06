mod action;
mod asset;
mod core;
mod execution;
mod program;
mod user;
mod vault;

pub use self::action::*;
pub use self::asset::*;
pub use self::core::*;
pub use self::execution::*;
pub use self::program::*;
pub use self::user::*;
pub use self::vault::*;

/// SPL Token program id (classic).
pub const TOKEN_PROGRAM_ID: solana_pubkey::Pubkey =
    solana_pubkey::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::{
        action::Ops,
        instructions::{
            AuthorizeActionArgs, CancelRedeemArgs, CollectFeesArgs, DepositArgs,
            InitializeAssetArgs, InitializeProgramArgs, InitializeVaultArgs, InstructionArgs,
            ManageArgs, ProcessWithdrawalsArgs, RedeemArgs, ReportNavArgs, RevokeActionArgs,
            SetNavAuthorityArgs, SetPauseFlagsArgs, SetStrategistArgs, SetVaultAccessArgs,
            SetWithdrawalAuthorityArgs, TransferProgramAuthorityArgs, TransferVaultAuthorityArgs,
            UpdateAssetArgs, UpdateVaultConfigArgs,
        },
        ID,
    };
    use solana_instruction::AccountMeta;
    use solana_pubkey::Pubkey;
    use solana_system_interface::program as system_program;
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
        let share_mint = roshi_interface::find_share_mint_address(&vault).0;
        let fee_collector = Pubkey::new_unique();
        let args = InitializeVaultArgs {
            tag: [1; 32],
            tag_len: 4,
            admin: Pubkey::new_unique().to_bytes(),
            strategist: Pubkey::new_unique().to_bytes(),
            nav_authority: Pubkey::new_unique().to_bytes(),
            withdrawal_authority: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            base_decimals: 6,
            base_oracle: roshi_interface::oracle::OracleConfig::default(),
            deposit_sub_account: 0,
            withdraw_sub_account: 1,
            fee_collector: fee_collector.to_bytes(),
            performance_fee_bps: 100,
            withdrawal_buffer_bps: 250,
            private: true,
            access_merkle_root: [2; 32],
        };

        let ix = initialize_vault(program_authority, program_config, payer, vault, args).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 9);
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
        assert_eq!(ix.accounts[4], AccountMeta::new_readonly(base_mint, false));
        assert_eq!(ix.accounts[5], AccountMeta::new(share_mint, false));
        assert_eq!(
            ix.accounts[6],
            AccountMeta::new_readonly(fee_collector, false)
        );
        assert_eq!(
            ix.accounts[7],
            AccountMeta::new_readonly(system_program::ID, false)
        );
        assert_eq!(
            ix.accounts[8],
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false)
        );

        let args: InitializeVaultArgs = decode_args(&ix.data);
        assert_eq!(args.tag, [1; 32]);
        assert_eq!(args.tag_len, 4);
        assert_eq!(args.base_mint, base_mint.to_bytes());
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
        let share_mint = Pubkey::new_unique();
        let asset_mint = Pubkey::new_unique();
        let asset_pda = Pubkey::new_unique();
        let proof = vec![[1; 32], [2; 32]];

        let ix = deposit(
            depositor,
            vault,
            source,
            custody,
            shares,
            share_mint,
            asset_mint,
            123,
            456,
            proof.clone(),
            vec![AccountMeta::new_readonly(asset_pda, false)],
        )
        .unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 8);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(depositor, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(source, false));
        assert_eq!(ix.accounts[3], AccountMeta::new(custody, false));
        assert_eq!(ix.accounts[4], AccountMeta::new(shares, false));
        assert_eq!(ix.accounts[5], AccountMeta::new(share_mint, false));
        assert_eq!(
            ix.accounts[6],
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false)
        );
        assert_eq!(ix.accounts[7], AccountMeta::new_readonly(asset_pda, false));

        let args: DepositArgs = decode_args(&ix.data);
        assert_eq!(args.asset_mint, asset_mint.to_bytes());
        assert_eq!(args.amount, 123);
        assert_eq!(args.min_shares_out, 456);
        assert_eq!(args.access_proof, proof);
    }

    #[test]
    fn builds_redeem_instruction() {
        let owner = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let share_source = Pubkey::new_unique();
        let share_mint = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let ticket = Pubkey::new_unique();

        let ix = redeem(
            owner,
            vault,
            share_source,
            share_mint,
            recipient,
            ticket,
            7,
            123,
        )
        .unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 8);
        assert_eq!(ix.accounts[0], AccountMeta::new(owner, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(share_source, false));
        assert_eq!(ix.accounts[3], AccountMeta::new(share_mint, false));
        assert_eq!(ix.accounts[4], AccountMeta::new_readonly(recipient, false));
        assert_eq!(ix.accounts[5], AccountMeta::new(ticket, false));
        assert_eq!(
            ix.accounts[6],
            AccountMeta::new_readonly(system_program::ID, false)
        );
        assert_eq!(
            ix.accounts[7],
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false)
        );

        let args: RedeemArgs = decode_args(&ix.data);
        assert_eq!(args.recipient_token_account, recipient.to_bytes());
        assert_eq!(args.ticket_index, 7);
        assert_eq!(args.shares, 123);
    }

    #[test]
    fn builds_cancel_redeem_instruction() {
        let owner = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let ticket = Pubkey::new_unique();
        let share_mint = Pubkey::new_unique();
        let share_dest = Pubkey::new_unique();

        let ix = cancel_redeem(owner, vault, ticket, share_mint, share_dest, 123).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 6);
        assert_eq!(ix.accounts[0], AccountMeta::new(owner, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(ticket, false));
        assert_eq!(ix.accounts[3], AccountMeta::new(share_mint, false));
        assert_eq!(ix.accounts[4], AccountMeta::new(share_dest, false));
        assert_eq!(
            ix.accounts[5],
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false)
        );

        let args: CancelRedeemArgs = decode_args(&ix.data);
        assert_eq!(args.min_shares_out, 123);
    }

    #[test]
    fn builds_process_withdrawals_instruction() {
        let withdrawal_authority = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let withdraw_sub_account = Pubkey::new_unique();
        let custody = Pubkey::new_unique();
        let share_mint = Pubkey::new_unique();
        let ticket = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let destination = Pubkey::new_unique();

        let ix = process_withdrawals(
            withdrawal_authority,
            vault,
            withdraw_sub_account,
            custody,
            share_mint,
            vec![(ticket, owner, destination)],
        )
        .unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 9);
        assert_eq!(
            ix.accounts[0],
            AccountMeta::new_readonly(withdrawal_authority, true)
        );
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(
            ix.accounts[2],
            AccountMeta::new_readonly(withdraw_sub_account, false)
        );
        assert_eq!(ix.accounts[3], AccountMeta::new(custody, false));
        assert_eq!(ix.accounts[4], AccountMeta::new_readonly(share_mint, false));
        assert_eq!(
            ix.accounts[5],
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false)
        );
        assert_eq!(ix.accounts[6], AccountMeta::new(ticket, false));
        assert_eq!(ix.accounts[7], AccountMeta::new(owner, false));
        assert_eq!(ix.accounts[8], AccountMeta::new(destination, false));

        let _args: ProcessWithdrawalsArgs = decode_args(&ix.data);
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

    #[test]
    fn builds_report_nav_instruction() {
        let nav_authority = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let share_mint = Pubkey::new_unique();
        let report_hash = [7; 32];

        let ix = report_nav(nav_authority, vault, share_mint, 123, report_hash).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(
            ix.accounts[0],
            AccountMeta::new_readonly(nav_authority, true)
        );
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new_readonly(share_mint, false));

        let args: ReportNavArgs = decode_args(&ix.data);
        assert_eq!(args.total_assets, 123);
        assert_eq!(args.report_hash, report_hash);
    }

    #[test]
    fn builds_collect_fees_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let fee_sub_account = Pubkey::new_unique();
        let custody = Pubkey::new_unique();
        let fee_collector = Pubkey::new_unique();

        let ix =
            collect_fees(admin, vault, 7, fee_sub_account, custody, fee_collector, 42).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 6);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(
            ix.accounts[2],
            AccountMeta::new_readonly(fee_sub_account, false)
        );
        assert_eq!(ix.accounts[3], AccountMeta::new(custody, false));
        assert_eq!(ix.accounts[4], AccountMeta::new(fee_collector, false));
        assert_eq!(
            ix.accounts[5],
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false)
        );

        let args: CollectFeesArgs = decode_args(&ix.data);
        assert_eq!(args.sub_account, 7);
        assert_eq!(args.amount, 42);
    }

    #[test]
    fn builds_authorize_action_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let action = Pubkey::new_unique();

        let ix = authorize_action(admin, vault, action, [9; 32], Ops::empty()).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 4);
        assert_eq!(ix.accounts[0], AccountMeta::new(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new_readonly(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(action, false));
        assert_eq!(
            ix.accounts[3],
            AccountMeta::new_readonly(system_program::ID, false)
        );

        let args: AuthorizeActionArgs = decode_args(&ix.data);
        assert_eq!(args.action_hash, [9; 32]);
        assert_eq!(args.ops, Ops::empty());
    }

    #[test]
    fn builds_revoke_action_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let action = Pubkey::new_unique();

        let ix = revoke_action(admin, vault, action, [9; 32]).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.accounts[0], AccountMeta::new(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new_readonly(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(action, false));

        let args: RevokeActionArgs = decode_args(&ix.data);
        assert_eq!(args.action_hash, [9; 32]);
    }

    #[test]
    fn builds_initialize_asset_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let asset = Pubkey::new_unique();
        let asset_mint = Pubkey::new_unique();
        let args = InitializeAssetArgs {
            asset_mint: asset_mint.to_bytes(),
            oracle: roshi_interface::oracle::OracleConfig::default(),
            asset_decimals: 9,
            enabled: true,
        };

        let ix = initialize_asset(admin, vault, asset, args).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 4);
        assert_eq!(ix.accounts[0], AccountMeta::new(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new_readonly(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(asset, false));
        assert_eq!(
            ix.accounts[3],
            AccountMeta::new_readonly(system_program::ID, false)
        );

        let args: InitializeAssetArgs = decode_args(&ix.data);
        assert_eq!(args.asset_mint, asset_mint.to_bytes());
        assert_eq!(args.asset_decimals, 9);
        assert!(args.enabled);
    }

    #[test]
    fn builds_update_asset_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let asset = Pubkey::new_unique();
        let args = UpdateAssetArgs {
            oracle: roshi_interface::oracle::OracleConfig::default(),
            enabled: false,
        };

        let ix = update_asset(admin, vault, asset, args).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new_readonly(vault, false));
        assert_eq!(ix.accounts[2], AccountMeta::new(asset, false));

        let args: UpdateAssetArgs = decode_args(&ix.data);
        assert!(!args.enabled);
    }

    #[test]
    fn builds_set_pause_flags_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();

        let ix = set_pause_flags(admin, vault, true, false, true).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 2);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));

        let args: SetPauseFlagsArgs = decode_args(&ix.data);
        assert!(args.deposits_paused);
        assert!(!args.withdrawals_paused);
        assert!(args.manage_paused);
    }

    #[test]
    fn builds_update_vault_config_instruction() {
        let admin = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let fee_collector = Pubkey::new_unique();
        let args = UpdateVaultConfigArgs {
            fee_collector: fee_collector.to_bytes(),
            deposit_sub_account: 2,
            withdraw_sub_account: 3,
            base_oracle: roshi_interface::oracle::OracleConfig::default(),
            performance_fee_bps: 150,
            withdrawal_buffer_bps: 300,
        };

        let ix = update_vault_config(admin, vault, args).unwrap();

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.accounts[0], AccountMeta::new_readonly(admin, true));
        assert_eq!(ix.accounts[1], AccountMeta::new(vault, false));
        assert_eq!(
            ix.accounts[2],
            AccountMeta::new_readonly(fee_collector, false)
        );

        let args: UpdateVaultConfigArgs = decode_args(&ix.data);
        assert_eq!(args.fee_collector, fee_collector.to_bytes());
        assert_eq!(args.deposit_sub_account, 2);
        assert_eq!(args.withdraw_sub_account, 3);
        assert_eq!(args.performance_fee_bps, 150);
        assert_eq!(args.withdrawal_buffer_bps, 300);
    }
}
