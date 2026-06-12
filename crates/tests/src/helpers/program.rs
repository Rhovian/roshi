use std::path::{Path, PathBuf};

use litesvm::LiteSVM;
use roshi::{state::program_config::ProgramConfig, ID};
use solana_instruction::Instruction;
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};

use super::transaction::{fund, send_ok_partially_signed};

pub fn program_so_path() -> PathBuf {
    std::env::var_os("ROSHI_PROGRAM_SO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../target/deploy/roshi.so"))
}

/// Load the program into a fresh SVM without initializing the program config.
/// Returns a funded keypair to drive (or attack) initialization.
pub fn setup_uninitialized_program() -> Option<(LiteSVM, Keypair)> {
    let program_so = program_so_path();
    if !Path::new(&program_so).exists() {
        eprintln!(
            "Skipping LiteSVM program setup; build the SBF first or set ROSHI_PROGRAM_SO: {}",
            program_so.display()
        );
        return None;
    }

    // Sigverify is disabled so initialization can carry the program account's
    // required signer flag without the program keypair, which lives in the
    // deployment repo. is_signer enforcement still comes from the message
    // header, so signer-gating tests are unaffected.
    let mut svm = LiteSVM::new().with_sigverify(false);
    svm.add_program_from_file(ID, program_so).unwrap();

    let authority = Keypair::new();
    fund(&mut svm, &authority);

    Some((svm, authority))
}

pub fn setup_program() -> Option<(LiteSVM, Keypair, Pubkey)> {
    let (mut svm, authority) = setup_uninitialized_program()?;

    let (config_pda, _) = ProgramConfig::find_address();
    let ix = initialize_program_ix(&authority.pubkey(), &config_pda, &authority.pubkey());

    send_ok_partially_signed(&mut svm, ix, &authority);

    Some((svm, authority, config_pda))
}

/// Metaplex Token Metadata program id (must match the program's vetted
/// constant).
pub const MPL_TOKEN_METADATA_ID: Pubkey =
    solana_pubkey::pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");

/// Like [`setup_program`], with the Metaplex Token Metadata program loaded
/// from the dumped fixture binary. Skips (None) when the binary is absent —
/// fetch it once with `just fetch-mpl`.
pub fn setup_program_with_metaplex() -> Option<(LiteSVM, Keypair, Pubkey)> {
    let (mut svm, authority, config_pda) = setup_program()?;

    let mpl_so = std::env::var_os("ROSHI_MPL_SO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("fixtures/mpl_token_metadata.so"));
    if !mpl_so.exists() {
        eprintln!(
            "Skipping Metaplex metadata test; fetch the binary with `just fetch-mpl` or set ROSHI_MPL_SO: {}",
            mpl_so.display()
        );
        return None;
    }
    svm.add_program_from_file(MPL_TOKEN_METADATA_ID, mpl_so)
        .unwrap();

    Some((svm, authority, config_pda))
}

/// Set the on-chain clock's unix timestamp (slot and the rest untouched).
pub fn set_clock_timestamp(svm: &mut LiteSVM, unix_timestamp: i64) {
    let mut clock: solana_sdk::clock::Clock = svm.get_sysvar();
    clock.unix_timestamp = unix_timestamp;
    svm.set_sysvar(&clock);
}

pub fn initialize_program_ix(
    payer: &Pubkey,
    config_pda: &Pubkey,
    authority: &Pubkey,
) -> Instruction {
    roshi_client::instruction::initialize_program(*payer, *config_pda, *authority).unwrap()
}
