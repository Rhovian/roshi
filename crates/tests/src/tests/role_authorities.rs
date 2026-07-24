//! Role-gated vault mutations.
//!
//! All of `set_*`/`transfer_vault_authority` funnel through the same
//! `update_writable_vault_as_admin` path (writable + admin-role + signer
//! checks). `set_strategist` is the flagship that exercises that path's RBAC
//! branches; the siblings then only need to prove each writes its own field and
//! leaves the rest of the vault untouched (asserted via full-struct equality,
//! since `Vault` is `PartialEq`).

use roshi::instructions::SetStrategistArgs;
use solana_instruction::{error::InstructionError, AccountMeta};
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{assert_instruction_error, fund, send, send_ok, setup_program, VaultBuilder};

#[test]
fn test_set_strategist() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let before = vault.load(&svm);
    let new_strategist = solana_pubkey::Pubkey::new_unique();

    let ix = roshi_client::instruction::set_strategist(
        vault.roles.admin.pubkey(),
        vault.address,
        new_strategist,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.strategist = new_strategist.to_bytes();
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_set_strategist_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let before = vault.load(&svm);

    // A funded outsider that holds no role on the vault.
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let ix = roshi_client::instruction::set_strategist(
        outsider.pubkey(),
        vault.address,
        solana_pubkey::Pubkey::new_unique(),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_set_strategist_requires_admin_signature() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let before = vault.load(&svm);

    // Correct admin key, but its account is not marked as a signer.
    let args = SetStrategistArgs {
        strategist: solana_pubkey::Pubkey::new_unique().to_bytes(),
    };
    let accounts = vec![
        AccountMeta::new_readonly(vault.roles.admin.pubkey(), false),
        AccountMeta::new(vault.address, false),
    ];
    let ix = roshi_client::instruction::new(accounts, &args).unwrap();

    let payer = Keypair::new();
    fund(&mut svm, &payer);
    assert_instruction_error(
        send(&mut svm, ix, &payer),
        InstructionError::MissingRequiredSignature,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_set_nav_authority() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let before = vault.load(&svm);
    let new_nav_authority = solana_pubkey::Pubkey::new_unique();

    let ix = roshi_client::instruction::set_nav_authority(
        vault.roles.admin.pubkey(),
        vault.address,
        new_nav_authority,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.nav_authority = new_nav_authority.to_bytes();
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_set_swap_authority() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let before = vault.load(&svm);
    let new_swap_authority = solana_pubkey::Pubkey::new_unique();

    let ix = roshi_client::instruction::set_swap_authority(
        vault.roles.admin.pubkey(),
        vault.address,
        new_swap_authority,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.swap_authority = new_swap_authority.to_bytes();
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_set_withdrawal_authority() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let before = vault.load(&svm);
    let new_withdrawal_authority = solana_pubkey::Pubkey::new_unique();

    let ix = roshi_client::instruction::set_withdrawal_authority(
        vault.roles.admin.pubkey(),
        vault.address,
        new_withdrawal_authority,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.withdrawal_authority = new_withdrawal_authority.to_bytes();
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_transfer_vault_authority_rotates_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let new_admin = Keypair::new();
    fund(&mut svm, &new_admin);

    // The current admin hands authority to the new admin; only `admin`
    // changes and the vault PDA still verifies against its tag + base mint.
    let before = vault.load(&svm);
    let ix = roshi_client::instruction::transfer_vault_authority(
        vault.roles.admin.pubkey(),
        vault.address,
        new_admin.pubkey(),
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.admin = new_admin.pubkey().to_bytes();
    assert_eq!(vault.load(&svm), expected);

    // The old admin can no longer act.
    let ix = roshi_client::instruction::set_strategist(
        vault.roles.admin.pubkey(),
        vault.address,
        solana_pubkey::Pubkey::new_unique(),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::IllegalOwner,
    );

    // The new admin can.
    let new_strategist = solana_pubkey::Pubkey::new_unique();
    let ix = roshi_client::instruction::set_strategist(
        new_admin.pubkey(),
        vault.address,
        new_strategist,
    )
    .unwrap();
    send_ok(&mut svm, ix, &new_admin);
    assert_eq!(vault.load(&svm).strategist, new_strategist.to_bytes());
}
