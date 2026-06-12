//! `write_down_fees` forgives accrued fee liability without moving tokens:
//! admin-gated, requires `0 < amount <= fees_payable`, leaves every other
//! vault field (including `total_assets`) untouched. It exists to unwedge
//! `report_nav` when losses ate into the fee cushion.

use litesvm::LiteSVM;
use roshi::error::RoshiError;
use solana_instruction::error::InstructionError;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, setup_program, TestVault,
    VaultBuilder,
};

/// Install a vault whose accrued fee liability is `fees_payable`.
fn install_vault_with_fees(svm: &mut LiteSVM, fees_payable: u64) -> TestVault {
    let vault = VaultBuilder::new().install(svm);
    let mut state = vault.load(svm);
    state.fees_payable = fees_payable;
    svm.set_account(
        vault.address,
        solana_sdk::account::Account {
            lamports: svm.minimum_balance_for_rent_exemption(roshi::state::vault::Vault::SPACE),
            data: wincode::serialize(&roshi::state::Account::Vault(state)).unwrap(),
            owner: roshi::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    vault
}

#[test]
fn test_write_down_fees_reduces_only_fees_payable() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = install_vault_with_fees(&mut svm, 1_000);
    fund(&mut svm, &vault.roles.admin);
    let before = vault.load(&svm);

    send_ok(
        &mut svm,
        roshi_client::instruction::write_down_fees(vault.roles.admin.pubkey(), vault.address, 400)
            .unwrap(),
        &vault.roles.admin,
    );

    let mut expected = before;
    expected.fees_payable = 600;
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_write_down_fees_can_clear_the_full_liability() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = install_vault_with_fees(&mut svm, 1_000);
    fund(&mut svm, &vault.roles.admin);

    send_ok(
        &mut svm,
        roshi_client::instruction::write_down_fees(
            vault.roles.admin.pubkey(),
            vault.address,
            1_000,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    assert_eq!(vault.load(&svm).fees_payable, 0);
}

#[test]
fn test_write_down_fees_rejects_zero_amount() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = install_vault_with_fees(&mut svm, 1_000);
    fund(&mut svm, &vault.roles.admin);

    let result = send(
        &mut svm,
        roshi_client::instruction::write_down_fees(vault.roles.admin.pubkey(), vault.address, 0)
            .unwrap(),
        &vault.roles.admin,
    );

    assert_roshi_error(result, RoshiError::InvalidWriteDownAmount);
}

#[test]
fn test_write_down_fees_rejects_amount_above_fees_payable() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = install_vault_with_fees(&mut svm, 1_000);
    fund(&mut svm, &vault.roles.admin);
    let before = vault.load(&svm);

    let result = send(
        &mut svm,
        roshi_client::instruction::write_down_fees(
            vault.roles.admin.pubkey(),
            vault.address,
            1_001,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    assert_roshi_error(result, RoshiError::InvalidWriteDownAmount);
    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_write_down_fees_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = install_vault_with_fees(&mut svm, 1_000);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let result = send(
        &mut svm,
        roshi_client::instruction::write_down_fees(outsider.pubkey(), vault.address, 400).unwrap(),
        &outsider,
    );

    assert_instruction_error(result, InstructionError::IllegalOwner);
}
