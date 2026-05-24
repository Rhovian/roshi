use std::path::{Path, PathBuf};

use litesvm::LiteSVM;
use roshi::{instructions::RoshiInstruction, state::program_config::ProgramConfig, ID};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};
use solana_transaction::{Address, Transaction};
use wincode::serialize;

pub fn program_so_path() -> PathBuf {
    std::env::var_os("ROSHI_PROGRAM_SO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../target/deploy/roshi.so"))
}

pub fn setup_program() -> Option<(LiteSVM, Keypair, Pubkey)> {
    let program_so = program_so_path();
    if !Path::new(&program_so).exists() {
        eprintln!(
            "Skipping LiteSVM program setup; build the SBF first or set ROSHI_PROGRAM_SO: {}",
            program_so.display()
        );
        return None;
    }

    let mut svm = LiteSVM::new();
    svm.add_program_from_file(ID, program_so).unwrap();

    let authority = Keypair::new();
    svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

    let (config_pda, _) = ProgramConfig::find_address();
    let ix = initialize_program_ix(&authority.pubkey(), &config_pda, &authority.pubkey());

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&Address::from(authority.pubkey())),
        &[&authority],
        blockhash,
    );
    svm.send_transaction(tx).unwrap();

    Some((svm, authority, config_pda))
}

pub fn initialize_program_ix(
    payer: &Pubkey,
    config_pda: &Pubkey,
    authority: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(*config_pda, false),
            AccountMeta::new_readonly(common::SYSTEM_PROGRAM, false),
        ],
        data: serialize(&RoshiInstruction::InitializeProgram {
            authority: authority.to_bytes(),
        })
        .unwrap(),
    }
}
