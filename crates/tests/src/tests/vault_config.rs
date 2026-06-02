//! Admin-gated, non-RBAC vault configuration: pause flags, access mode, and the
//! bulk config update. Like the role setters these route through
//! `update_writable_vault_as_admin`; the negative tests confirm each freshly
//! wired handler is actually gated and validated.

use roshi::{error::RoshiError, instructions::UpdateVaultConfigArgs, oracle::OracleConfig};
use solana_instruction::error::InstructionError;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, setup_program, VaultBuilder,
};

#[test]
fn test_set_pause_flags() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let before = vault.load(&svm);
    let ix = roshi_client::instruction::set_pause_flags(
        vault.roles.admin.pubkey(),
        vault.address,
        true,
        false,
        true,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.set_deposits_paused(true);
    expected.set_withdrawals_paused(false);
    expected.set_manage_paused(true);
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_set_pause_flags_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let before = vault.load(&svm);

    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let ix = roshi_client::instruction::set_pause_flags(
        outsider.pubkey(),
        vault.address,
        true,
        true,
        true,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_set_vault_access_toggles_private_and_public() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    // Public -> private with an access root.
    let before = vault.load(&svm);
    let root = [7; 32];
    let ix = roshi_client::instruction::set_vault_access(
        vault.roles.admin.pubkey(),
        vault.address,
        true,
        root,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.set_private(true);
    expected.access_merkle_root = root;
    assert_eq!(vault.load(&svm), expected);

    // Private -> public, root cleared.
    let before = vault.load(&svm);
    let ix = roshi_client::instruction::set_vault_access(
        vault.roles.admin.pubkey(),
        vault.address,
        false,
        [0; 32],
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.set_private(false);
    expected.access_merkle_root = [0; 32];
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_update_vault_config() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let before = vault.load(&svm);
    let new_fee_collector = solana_pubkey::Pubkey::new_unique();
    let args = UpdateVaultConfigArgs {
        fee_collector: new_fee_collector.to_bytes(),
        deposit_sub_account: 4,
        withdraw_sub_account: 5,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 150,
        withdrawal_buffer_bps: 300,
        max_change_bps: 600,
        min_update_interval: 120,
    };
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        args,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.fee_collector = new_fee_collector.to_bytes();
    expected.deposit_sub_account = 4;
    expected.withdraw_sub_account = 5;
    expected.performance_fee_bps = 150;
    expected.withdrawal_buffer_bps = 300;
    expected.max_change_bps = 600;
    expected.min_update_interval = 120;
    assert_eq!(vault.load(&svm), expected);
}

#[test]
fn test_update_vault_config_rejects_invalid_bps() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let before = vault.load(&svm);

    // > 100% performance fee must be rejected by validate_state on store.
    let args = UpdateVaultConfigArgs {
        fee_collector: vault.fee_collector.to_bytes(),
        deposit_sub_account: 0,
        withdraw_sub_account: 1,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 10_001,
        withdrawal_buffer_bps: 0,
        max_change_bps: 0,
        min_update_interval: 0,
    };
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        args,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidBps,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_update_vault_config_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let before = vault.load(&svm);

    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let args = UpdateVaultConfigArgs {
        fee_collector: solana_pubkey::Pubkey::new_unique().to_bytes(),
        deposit_sub_account: 0,
        withdraw_sub_account: 1,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 100,
        withdrawal_buffer_bps: 250,
        max_change_bps: 500,
        min_update_interval: 60,
    };
    let ix = roshi_client::instruction::update_vault_config(outsider.pubkey(), vault.address, args)
        .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert_eq!(vault.load(&svm), before);
}
