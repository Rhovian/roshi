//! Admin-gated, non-RBAC vault configuration: pause flags, access mode, and the
//! bulk config update. Like the role setters these route through
//! `update_writable_vault_as_admin`; the negative tests confirm each freshly
//! wired handler is actually gated and validated.

use roshi::{error::RoshiError, instructions::UpdateVaultConfigArgs, oracle::OracleConfig};
use solana_instruction::error::InstructionError;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, set_token_account,
    setup_program, VaultBuilder,
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
    let new_treasury = solana_pubkey::Pubkey::new_unique();
    set_token_account(
        &mut svm,
        new_treasury,
        &vault.base_mint,
        &solana_pubkey::Pubkey::new_unique(),
        0,
    );
    let args = UpdateVaultConfigArgs {
        treasury: new_treasury.to_bytes(),
        deposit_sub_account: 4,
        withdraw_sub_account: 5,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 150,
        withdrawal_buffer_bps: 300,
        external_enabled: true,
    };
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        args,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.treasury = new_treasury.to_bytes();
    expected.deposit_sub_account = 4;
    expected.withdraw_sub_account = 5;
    expected.performance_fee_bps = 150;
    expected.withdrawal_buffer_bps = 300;
    expected.set_external_enabled(true);
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
    set_token_account(
        &mut svm,
        vault.treasury,
        &vault.base_mint,
        &solana_pubkey::Pubkey::new_unique(),
        0,
    );

    // > 100% performance fee must be rejected by validate_state on store.
    let args = UpdateVaultConfigArgs {
        treasury: vault.treasury.to_bytes(),
        deposit_sub_account: 0,
        withdraw_sub_account: 1,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 10_001,
        withdrawal_buffer_bps: 0,
        external_enabled: false,
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
    let treasury = solana_pubkey::Pubkey::new_unique();
    set_token_account(
        &mut svm,
        treasury,
        &vault.base_mint,
        &solana_pubkey::Pubkey::new_unique(),
        0,
    );
    let args = UpdateVaultConfigArgs {
        treasury: treasury.to_bytes(),
        deposit_sub_account: 0,
        withdraw_sub_account: 1,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 100,
        withdrawal_buffer_bps: 250,
        external_enabled: false,
    };
    let ix = roshi_client::instruction::update_vault_config(outsider.pubkey(), vault.address, args)
        .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_update_vault_config_rejects_treasury_for_wrong_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let before = vault.load(&svm);

    let new_treasury = solana_pubkey::Pubkey::new_unique();
    let wrong_mint = solana_pubkey::Pubkey::new_unique();
    set_token_account(
        &mut svm,
        new_treasury,
        &wrong_mint,
        &solana_pubkey::Pubkey::new_unique(),
        0,
    );
    let args = UpdateVaultConfigArgs {
        treasury: new_treasury.to_bytes(),
        deposit_sub_account: 0,
        withdraw_sub_account: 1,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 100,
        withdrawal_buffer_bps: 250,
        external_enabled: false,
    };
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        args,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidTokenAccount,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_update_vault_config_allows_withdraw_subaccount_rotation_with_liabilities() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let mut before = vault.load(&svm);
    before.fees_payable = 1;
    before.pending_withdrawal_assets = 1;
    svm.set_account(
        vault.address,
        solana_sdk::account::Account {
            lamports: svm.minimum_balance_for_rent_exemption(roshi::state::vault::Vault::SPACE),
            data: wincode::serialize(&roshi::state::Account::Vault(before)).unwrap(),
            owner: roshi::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let new_treasury = solana_pubkey::Pubkey::new_unique();
    set_token_account(
        &mut svm,
        new_treasury,
        &vault.base_mint,
        &solana_pubkey::Pubkey::new_unique(),
        0,
    );
    let args = UpdateVaultConfigArgs {
        treasury: new_treasury.to_bytes(),
        deposit_sub_account: 0,
        withdraw_sub_account: 5,
        base_oracle: OracleConfig::default(),
        performance_fee_bps: 100,
        withdrawal_buffer_bps: 250,
        external_enabled: false,
    };
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        args,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.treasury = new_treasury.to_bytes();
    expected.withdraw_sub_account = 5;
    assert_eq!(vault.load(&svm), expected);
}
