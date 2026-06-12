//! `register_external_destination` / `revoke_external_destination` manage the
//! admin-authorized venue registry for `invest_external`: registration binds
//! a base-mint token account to the PDA for `(vault, token_account)`, revoke
//! closes it back to the admin. Both are admin-gated.

use litesvm::LiteSVM;
use roshi::{
    error::RoshiError,
    state::{external_destination::ExternalDestination, Account as RoshiAccount},
    ID,
};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};
use wincode::deserialize;

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, set_token_account,
    setup_program, TestVault, VaultBuilder,
};

/// Install a base-mint token account usable as an external destination.
fn base_destination(svm: &mut LiteSVM, vault: &TestVault) -> Pubkey {
    let destination = Pubkey::new_unique();
    set_token_account(svm, destination, &vault.base_mint, &Pubkey::new_unique(), 0);
    destination
}

fn register(svm: &mut LiteSVM, vault: &TestVault, destination: Pubkey) -> Pubkey {
    let (pda, _) = ExternalDestination::find_address(&vault.address, &destination);
    send_ok(
        svm,
        roshi_client::instruction::register_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            destination,
            pda,
        )
        .unwrap(),
        &vault.roles.admin,
    );
    pda
}

#[test]
fn test_register_external_destination() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let destination = base_destination(&mut svm, &vault);
    let (pda, bump) = ExternalDestination::find_address(&vault.address, &destination);
    assert!(svm.get_account(&pda).is_none());

    register(&mut svm, &vault, destination);

    let account = svm.get_account(&pda).unwrap();
    assert_eq!(account.owner, ID);
    assert_eq!(account.data.len(), ExternalDestination::SPACE);

    let RoshiAccount::ExternalDestination(registered) = deserialize(&account.data).unwrap() else {
        panic!("expected external destination account");
    };
    assert_eq!(registered.vault, vault.address.to_bytes());
    assert_eq!(registered.token_account, destination.to_bytes());
    assert_eq!(registered.bump, bump);
}

#[test]
fn test_register_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let destination = base_destination(&mut svm, &vault);
    let (pda, _) = ExternalDestination::find_address(&vault.address, &destination);

    let result = send(
        &mut svm,
        roshi_client::instruction::register_external_destination(
            outsider.pubkey(),
            vault.address,
            destination,
            pda,
        )
        .unwrap(),
        &outsider,
    );

    assert_instruction_error(result, InstructionError::IllegalOwner);
}

#[test]
fn test_register_rejects_destination_with_wrong_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let destination = Pubkey::new_unique();
    set_token_account(
        &mut svm,
        destination,
        &Pubkey::new_unique(),
        &Pubkey::new_unique(),
        0,
    );
    let (pda, _) = ExternalDestination::find_address(&vault.address, &destination);

    let result = send(
        &mut svm,
        roshi_client::instruction::register_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            destination,
            pda,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    assert_roshi_error(result, RoshiError::InvalidTokenAccount);
}

#[test]
fn test_register_rejects_non_canonical_pda() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let destination = base_destination(&mut svm, &vault);
    let other_destination = base_destination(&mut svm, &vault);
    let (other_pda, _) = ExternalDestination::find_address(&vault.address, &other_destination);

    let result = send(
        &mut svm,
        roshi_client::instruction::register_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            destination,
            other_pda,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    assert_instruction_error(result, InstructionError::InvalidSeeds);
}

#[test]
fn test_register_rejects_already_registered_destination() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let destination = base_destination(&mut svm, &vault);
    let pda = register(&mut svm, &vault, destination);

    let result = send(
        &mut svm,
        roshi_client::instruction::register_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            destination,
            pda,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    assert_instruction_error(result, InstructionError::AccountAlreadyInitialized);
}

#[test]
fn test_revoke_external_destination_closes_and_refunds_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let destination = base_destination(&mut svm, &vault);
    let pda = register(&mut svm, &vault, destination);
    let admin_balance_after_register = svm.get_balance(&vault.roles.admin.pubkey()).unwrap();

    send_ok(
        &mut svm,
        roshi_client::instruction::revoke_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            pda,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    assert!(svm.get_account(&pda).is_none());
    // Rent came back: the refund exceeds the transaction fee.
    assert!(svm.get_balance(&vault.roles.admin.pubkey()).unwrap() > admin_balance_after_register);
}

#[test]
fn test_revoke_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let destination = base_destination(&mut svm, &vault);
    let pda = register(&mut svm, &vault, destination);

    let result = send(
        &mut svm,
        roshi_client::instruction::revoke_external_destination(
            outsider.pubkey(),
            vault.address,
            pda,
        )
        .unwrap(),
        &outsider,
    );

    assert_instruction_error(result, InstructionError::IllegalOwner);
    assert!(svm.get_account(&pda).is_some());
}

#[test]
fn test_revoke_rejects_foreign_account() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let not_a_registration = Pubkey::new_unique();
    set_token_account(
        &mut svm,
        not_a_registration,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );

    let result = send(
        &mut svm,
        roshi_client::instruction::revoke_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            not_a_registration,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    assert_instruction_error(result, InstructionError::IllegalOwner);
}
