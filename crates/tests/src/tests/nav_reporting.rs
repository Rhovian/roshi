//! Trusted NAV reporting tests. These cover the authority boundary, report
//! commitment storage, and the per-report change guardrail.

use roshi::error::RoshiError;
use solana_instruction::error::InstructionError;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, setup_program, VaultBuilder,
};

fn report_nav_ix(
    nav_authority: solana_pubkey::Pubkey,
    vault: solana_pubkey::Pubkey,
    total_assets: u64,
    report_hash: [u8; 32],
) -> solana_instruction::Instruction {
    roshi_client::instruction::report_nav(nav_authority, vault, total_assets, report_hash).unwrap()
}

#[test]
fn test_report_nav_accepts_first_report() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let report_hash = [1; 32];
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        1_000_000,
        report_hash,
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_000_000);
    assert_eq!(state.last_report_hash, report_hash);
}

#[test]
fn test_report_nav_rejects_non_nav_authority() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let before = vault.load(&svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let ix = report_nav_ix(outsider.pubkey(), vault.address, 1_000_000, [1; 32]);
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_report_nav_rejects_zero_report_hash() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let before = vault.load(&svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        1_000_000,
        [0; 32],
    );
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        RoshiError::InvalidVaultState,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_report_nav_enforces_max_change_after_first_report() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().fees(100, 250, 500).install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        1_050_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let before = vault.load(&svm);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        1_102_501,
        [3; 32],
    );
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        RoshiError::InvalidVaultState,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_report_nav_allows_first_report_without_delta_baseline() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().fees(100, 250, 0).install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_000_000);
    assert_eq!(state.last_report_hash, [1; 32]);
}
