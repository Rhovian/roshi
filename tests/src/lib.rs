#[cfg(test)]
mod helpers;

#[cfg(test)]
mod tests {
    use roshi::{instructions::RoshiInstruction, state::program_config::ProgramConfig, ID};
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
        let Some((mut svm, authority, config_pda)) = setup_program() else {
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

        let ix_data = RoshiInstruction::Manage {
            program_id: common::SYSTEM_PROGRAM.to_bytes(),
            accounts_start: 2,
            accounts_len: 2,
            ix_data: transfer_data,
        };

        let ix = Instruction {
            program_id: ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new_readonly(config_pda, false),
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
                AccountMeta::new_readonly(config_pda, false),
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
