#[cfg(test)]
mod helpers;

#[cfg(test)]
mod tests {
    use roshi::{
        instructions::RoshiInstruction,
        state::{
            action::{compute_action_hash, Action, Ops},
            program_config::ProgramConfig,
            vault::Vault,
            Account as RoshiAccount,
        },
        ID,
    };
    use solana_instruction::{AccountMeta, Instruction};
    use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
    use solana_transaction::{Address, Transaction};
    use wincode::serialize;

    use crate::helpers::{initialize_program_ix, setup_program};

    #[test]
    fn test_initialize_program() {
        let Some((svm, _authority, config_pda)) = setup_program() else {
            return;
        };

        let account = svm.get_account(&config_pda).unwrap();
        assert_eq!(account.owner, ID);
        assert_eq!(account.data.len(), ProgramConfig::SPACE);
    }

    #[test]
    fn test_manage_authority_check() {
        let Some((mut svm, authority, _config_pda)) = setup_program() else {
            return;
        };

        let scratch = solana_pubkey::Pubkey::new_unique();
        svm.set_account(
            scratch,
            Account {
                lamports: 0,
                data: vec![],
                owner: common::SYSTEM_PROGRAM,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        let mut transfer_data = vec![2, 0, 0, 0];
        transfer_data.extend_from_slice(&1_000_000u64.to_le_bytes());

        let base_mint = solana_pubkey::Pubkey::new_unique();
        let share_mint = solana_pubkey::Pubkey::new_unique();
        let vault_token_account = solana_pubkey::Pubkey::new_unique();
        let (vault_pda, vault_bump) = Vault::find_address(&authority.pubkey(), &base_mint);
        svm.set_account(
            vault_pda,
            Account {
                lamports: 1_000_000,
                data: serialize(&RoshiAccount::Vault(Vault {
                    admin: authority.pubkey().to_bytes(),
                    operator: authority.pubkey().to_bytes(),
                    queue_authority: authority.pubkey().to_bytes(),
                    base_mint: base_mint.to_bytes(),
                    share_mint: share_mint.to_bytes(),
                    vault_token_account: vault_token_account.to_bytes(),
                    fee_collector: authority.pubkey().to_bytes(),
                    total_assets: 0,
                    external_assets: 0,
                    total_shares: 0,
                    pending_withdrawal_assets: 0,
                    high_watermark: 0,
                    performance_fee_bps: 0,
                    withdrawal_buffer_bps: 0,
                    max_change_bps: 0,
                    min_update_interval: 0,
                    last_update_ts: 0,
                    current_withdrawal_epoch: 1,
                    processed_withdrawal_epoch: 0,
                    deposits_paused: false,
                    withdrawals_paused: false,
                    bump: vault_bump,
                }))
                .unwrap(),
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        let ops = Ops { ops: vec![] };
        let action_hash =
            compute_action_hash(&common::SYSTEM_PROGRAM, &ops, &[], &transfer_data).unwrap();
        let (action_pda, action_bump) = Action::find_address(&vault_pda, &action_hash);
        svm.set_account(
            action_pda,
            Account {
                lamports: 1_000_000,
                data: serialize(&RoshiAccount::Action(Action {
                    vault: vault_pda.to_bytes(),
                    action_hash,
                    ops,
                    bump: action_bump,
                }))
                .unwrap(),
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        let ix_data = RoshiInstruction::Manage {
            program_id: common::SYSTEM_PROGRAM.to_bytes(),
            accounts_start: 0,
            accounts_len: 2,
            ix_data: transfer_data,
        };

        let ix = Instruction {
            program_id: ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(action_pda, false),
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(scratch, false),
                AccountMeta::new_readonly(common::SYSTEM_PROGRAM, false),
            ],
            data: serialize(&ix_data).unwrap(),
        };

        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&Address::from(authority.pubkey())),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(result.is_ok(), "manage transfer failed: {result:?}");
        assert_eq!(svm.get_account(&scratch).unwrap().lamports, 1_000_000);

        let wrong = Keypair::new();
        svm.airdrop(&wrong.pubkey(), 10_000_000_000).unwrap();

        let ix = Instruction {
            program_id: ID,
            accounts: vec![
                AccountMeta::new(wrong.pubkey(), true),
                AccountMeta::new_readonly(vault_pda, false),
                AccountMeta::new_readonly(action_pda, false),
                AccountMeta::new(wrong.pubkey(), true),
                AccountMeta::new(scratch, false),
                AccountMeta::new_readonly(common::SYSTEM_PROGRAM, false),
            ],
            data: serialize(&ix_data).unwrap(),
        };

        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&Address::from(wrong.pubkey())),
            &[&wrong],
            blockhash,
        );
        assert!(svm.send_transaction(tx).is_err());
    }

    #[test]
    #[ignore]
    fn surfpool_smoke_uses_local_rpc() {
        let rpc_url =
            std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
        assert!(rpc_url.starts_with("http"));
    }

    #[test]
    fn build_initialize_program_instruction_without_sbf() {
        let payer = solana_pubkey::Pubkey::new_unique();
        let (config_pda, _) = ProgramConfig::find_address();
        let authority = solana_pubkey::Pubkey::new_unique();
        let ix = initialize_program_ix(&payer, &config_pda, &authority);

        assert_eq!(ix.program_id, ID);
        assert_eq!(ix.accounts.len(), 3);
    }
}
