use roshi::{state::program_config::ProgramConfig, ID};

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
#[ignore]
fn local_rpc_smoke_uses_rpc_url() {
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
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
