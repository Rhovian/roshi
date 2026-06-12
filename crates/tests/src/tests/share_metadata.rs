//! `set_share_metadata`: admin-gated Metaplex Token Metadata for the share
//! mint. The vault PDA signs as mint authority and is the metadata update
//! authority, so renames only ever go through this instruction. Display only.
//!
//! The create/update tests need the dumped Metaplex binary
//! (`just fetch-mpl`); they skip when it is absent. The rejection tests fire
//! before the CPI and run everywhere.

use litesvm::LiteSVM;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, fund, send, send_ok, setup_program, setup_program_with_metaplex,
    TestVault, VaultBuilder, MPL_TOKEN_METADATA_ID,
};

fn metadata_address(share_mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"metadata",
            MPL_TOKEN_METADATA_ID.as_ref(),
            share_mint.as_ref(),
        ],
        &MPL_TOKEN_METADATA_ID,
    )
    .0
}

fn set_metadata_ix(
    vault: &TestVault,
    signer: Pubkey,
    metadata: Pubkey,
    name: &str,
) -> solana_instruction::Instruction {
    roshi_client::instruction::set_share_metadata(
        signer,
        vault.address,
        vault.share_mint,
        metadata,
        MPL_TOKEN_METADATA_ID,
        name.to_string(),
        "ROSHI".to_string(),
        "https://roshi.example/shares.json".to_string(),
    )
    .unwrap()
}

fn install_vault(svm: &mut LiteSVM) -> TestVault {
    let builder = VaultBuilder::new();
    builder.install_mints(svm);
    let vault = builder.install(svm);
    fund(svm, &vault.roles.admin);
    vault
}

#[test]
fn test_set_share_metadata_creates_then_updates() {
    let Some((mut svm, ..)) = setup_program_with_metaplex() else {
        return;
    };
    let vault = install_vault(&mut svm);
    let metadata = metadata_address(&vault.share_mint);
    assert!(svm.get_account(&metadata).is_none());

    send_ok(
        &mut svm,
        set_metadata_ix(&vault, vault.roles.admin.pubkey(), metadata, "Roshi Shares"),
        &vault.roles.admin,
    );

    let account = svm.get_account(&metadata).unwrap();
    assert_eq!(account.owner, MPL_TOKEN_METADATA_ID);
    let contains = |needle: &[u8]| {
        account
            .data
            .windows(needle.len())
            .any(|window| window == needle)
    };
    assert!(contains(b"Roshi Shares"));
    assert!(contains(b"ROSHI"));
    // The vault PDA is the recorded update authority.
    assert!(contains(vault.address.as_ref()));

    // Second call detects the existing account and updates in place.
    svm.expire_blockhash();
    send_ok(
        &mut svm,
        set_metadata_ix(&vault, vault.roles.admin.pubkey(), metadata, "Roshi Prime"),
        &vault.roles.admin,
    );
    let account = svm.get_account(&metadata).unwrap();
    assert!(account
        .data
        .windows(b"Roshi Prime".len())
        .any(|window| window == b"Roshi Prime"));
}

#[test]
fn test_set_share_metadata_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = install_vault(&mut svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let metadata = metadata_address(&vault.share_mint);

    assert_instruction_error(
        send(
            &mut svm,
            set_metadata_ix(&vault, outsider.pubkey(), metadata, "Imposter"),
            &outsider,
        ),
        InstructionError::IllegalOwner,
    );
}

#[test]
fn test_set_share_metadata_rejects_wrong_metadata_pda() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = install_vault(&mut svm);

    assert_instruction_error(
        send(
            &mut svm,
            set_metadata_ix(
                &vault,
                vault.roles.admin.pubkey(),
                Pubkey::new_unique(),
                "Roshi Shares",
            ),
            &vault.roles.admin,
        ),
        InstructionError::InvalidSeeds,
    );
}

#[test]
fn test_set_share_metadata_rejects_wrong_metadata_program() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = install_vault(&mut svm);
    let metadata = metadata_address(&vault.share_mint);

    let ix = roshi_client::instruction::set_share_metadata(
        vault.roles.admin.pubkey(),
        vault.address,
        vault.share_mint,
        metadata,
        Pubkey::new_unique(),
        "Roshi Shares".to_string(),
        "ROSHI".to_string(),
        "https://roshi.example/shares.json".to_string(),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::IncorrectProgramId,
    );
}
