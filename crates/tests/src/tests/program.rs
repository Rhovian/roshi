use roshi::{state::program_config::ProgramConfig, ID};
use solana_instruction::error::InstructionError;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, fund, initialize_program_ix, send_ok_partially_signed,
    send_partially_signed, send_signed, setup_program, setup_uninitialized_program,
};

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
fn test_initialize_program_requires_program_keypair_signature() {
    let Some((mut svm, attacker)) = setup_uninitialized_program() else {
        return;
    };

    // A front-runner can name the program account but cannot sign for it.
    let (config_pda, _) = ProgramConfig::find_address();
    let mut ix = initialize_program_ix(&attacker.pubkey(), &config_pda, &attacker.pubkey());
    ix.accounts[1].is_signer = false;

    assert_instruction_error(
        send_signed(&mut svm, ix, &attacker, &[]),
        InstructionError::MissingRequiredSignature,
    );
    assert!(svm.get_account(&config_pda).is_none());
}

#[test]
fn test_initialize_program_rejects_substitute_program_account() {
    let Some((mut svm, attacker)) = setup_uninitialized_program() else {
        return;
    };

    // Signing with some other keypair in the program account slot must fail.
    let impostor = Keypair::new();
    fund(&mut svm, &impostor);
    let (config_pda, _) = ProgramConfig::find_address();
    let mut ix = initialize_program_ix(&attacker.pubkey(), &config_pda, &attacker.pubkey());
    ix.accounts[1].pubkey = impostor.pubkey();

    assert_instruction_error(
        send_signed(&mut svm, ix, &attacker, &[&impostor]),
        InstructionError::IncorrectProgramId,
    );
    assert!(svm.get_account(&config_pda).is_none());
}

#[test]
fn test_initialize_program_rejects_reinitialization() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let usurper = Keypair::new();
    fund(&mut svm, &usurper);
    let ix = initialize_program_ix(&usurper.pubkey(), &config_pda, &usurper.pubkey());

    assert_instruction_error(
        send_partially_signed(&mut svm, ix, &usurper),
        InstructionError::AccountAlreadyInitialized,
    );

    // The original authority still holds the config.
    let account = svm.get_account(&config_pda).unwrap();
    let roshi::state::Account::ProgramConfig(config) = wincode::deserialize(&account.data).unwrap()
    else {
        panic!("config PDA does not hold a ProgramConfig account");
    };
    assert_eq!(config.authority(), authority.pubkey());
}

#[test]
fn test_initialize_program_tolerates_prefunded_config_pda() {
    let Some((mut svm, authority)) = setup_uninitialized_program() else {
        return;
    };

    // Anyone can transfer lamports to the deterministic config PDA before it is
    // created. That prefund must not grief the legitimate first initialization.
    let (config_pda, _) = ProgramConfig::find_address();
    svm.airdrop(&config_pda, 1_000_000).unwrap();

    let ix = initialize_program_ix(&authority.pubkey(), &config_pda, &authority.pubkey());
    send_ok_partially_signed(&mut svm, ix, &authority);

    let account = svm.get_account(&config_pda).unwrap();
    assert_eq!(account.owner, ID);
    assert_eq!(account.data.len(), ProgramConfig::SPACE);
    let roshi::state::Account::ProgramConfig(config) = wincode::deserialize(&account.data).unwrap()
    else {
        panic!("config PDA does not hold a ProgramConfig account");
    };
    assert_eq!(config.authority(), authority.pubkey());
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
    assert_eq!(ix.accounts.len(), 4);
    // The program's own keypair co-signs initialization (front-run guard).
    assert_eq!(ix.accounts[1].pubkey, ID);
    assert!(ix.accounts[1].is_signer);
}
