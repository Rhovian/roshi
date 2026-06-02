//! `initialize_asset` / `update_asset` manage the per-vault config for a
//! supported non-base deposit asset (custody account, oracle, decimals, deposit
//! limit, enabled). Both are admin-gated; the custody/oracle are stored as
//! config and validated on-chain at deposit time. `initialize_asset` creates
//! the Asset PDA, `update_asset` mutates its non-immutable fields.

use litesvm::LiteSVM;
use roshi::{
    error::RoshiError,
    instructions::{InitializeAssetArgs, UpdateAssetArgs},
    oracle::{OracleConfig, PythOracleConfig},
    state::{asset::Asset, Account as RoshiAccount},
    ID,
};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};
use wincode::deserialize;

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, setup_program, TestVault,
    VaultBuilder,
};

fn init_args(asset_mint: Pubkey, custody: Pubkey) -> InitializeAssetArgs {
    InitializeAssetArgs {
        asset_mint: asset_mint.to_bytes(),
        custody_token_account: custody.to_bytes(),
        oracle: OracleConfig::default(),
        asset_decimals: 9,
        max_price_change_bps: 250,
        deposit_limit: 1_000_000,
        enabled: true,
    }
}

fn update_args(custody: Pubkey) -> UpdateAssetArgs {
    UpdateAssetArgs {
        custody_token_account: custody.to_bytes(),
        oracle: OracleConfig::default(),
        max_price_change_bps: 300,
        deposit_limit: 0,
        enabled: false,
    }
}

/// Initialize an enabled asset under `vault` and return its PDA.
fn create_asset(svm: &mut LiteSVM, vault: &TestVault, asset_mint: Pubkey) -> Pubkey {
    fund(svm, &vault.roles.admin);
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);
    send_ok(
        svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_pda,
            init_args(asset_mint, Pubkey::new_unique()),
        )
        .unwrap(),
        &vault.roles.admin,
    );
    asset_pda
}

fn load_asset(svm: &LiteSVM, asset: Pubkey) -> Asset {
    let account = svm.get_account(&asset).unwrap();
    let RoshiAccount::Asset(asset) = deserialize(&account.data).unwrap() else {
        panic!("expected asset account");
    };
    asset
}

#[test]
fn test_initialize_asset() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let asset_mint = Pubkey::new_unique();
    let custody = Pubkey::new_unique();
    let (asset_pda, bump) = Asset::find_address(&vault.address, &asset_mint);
    assert!(svm.get_account(&asset_pda).is_none());

    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_pda,
            init_args(asset_mint, custody),
        )
        .unwrap(),
        &vault.roles.admin,
    );

    let account = svm.get_account(&asset_pda).unwrap();
    assert_eq!(account.owner, ID);
    assert_eq!(account.data.len(), Asset::SPACE);

    let asset = load_asset(&svm, asset_pda);
    assert_eq!(asset.vault, vault.address.to_bytes());
    assert_eq!(asset.asset_mint, asset_mint.to_bytes());
    assert_eq!(asset.custody_token_account, custody.to_bytes());
    assert_eq!(asset.asset_decimals, 9);
    assert_eq!(asset.max_price_change_bps, 250);
    assert_eq!(asset.deposit_limit, 1_000_000);
    assert_eq!(asset.enabled(), Ok(true));
    assert_eq!(asset.bump, bump);
}

#[test]
fn test_initialize_asset_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let asset_mint = Pubkey::new_unique();
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);
    let ix = roshi_client::instruction::initialize_asset(
        outsider.pubkey(),
        vault.address,
        asset_pda,
        init_args(asset_mint, Pubkey::new_unique()),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert!(svm.get_account(&asset_pda).is_none());
}

#[test]
fn test_initialize_asset_rejects_base_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    // The vault base mint is not a non-base deposit asset.
    let base_mint = vault.base_mint;
    let (asset_pda, _) = Asset::find_address(&vault.address, &base_mint);
    let ix = roshi_client::instruction::initialize_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        asset_pda,
        init_args(base_mint, Pubkey::new_unique()),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::InvalidArgument,
    );

    assert!(svm.get_account(&asset_pda).is_none());
}

#[test]
fn test_initialize_asset_rejects_invalid_bps() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let asset_mint = Pubkey::new_unique();
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);
    let mut args = init_args(asset_mint, Pubkey::new_unique());
    args.max_price_change_bps = 10_001;

    let ix = roshi_client::instruction::initialize_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        asset_pda,
        args,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidBps,
    );

    assert!(svm.get_account(&asset_pda).is_none());
}

#[test]
fn test_initialize_asset_rejects_mismatched_seeds() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let asset_mint = Pubkey::new_unique();
    // The asset account does not match the PDA for asset_mint.
    let wrong_asset = Pubkey::new_unique();
    let ix = roshi_client::instruction::initialize_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        wrong_asset,
        init_args(asset_mint, Pubkey::new_unique()),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::InvalidSeeds,
    );
}

#[test]
fn test_initialize_asset_rejects_duplicate() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let asset_mint = Pubkey::new_unique();
    let asset_pda = create_asset(&mut svm, &vault, asset_mint);

    svm.expire_blockhash();
    let ix = roshi_client::instruction::initialize_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        asset_pda,
        init_args(asset_mint, Pubkey::new_unique()),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::AccountAlreadyInitialized,
    );
}

#[test]
fn test_update_asset() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let asset_pda = create_asset(&mut svm, &vault, Pubkey::new_unique());

    // Use a distinct, valid oracle (Pyth) so the assertion proves the oracle
    // field is actually replaced (the asset was created with the default
    // Switchboard config).
    let before = load_asset(&svm, asset_pda);
    let new_custody = Pubkey::new_unique();
    let new_oracle = OracleConfig::pyth(PythOracleConfig::new([4; 32], 8, 30, 250));
    assert_ne!(new_oracle, before.oracle);
    let args = UpdateAssetArgs {
        custody_token_account: new_custody.to_bytes(),
        oracle: new_oracle,
        max_price_change_bps: 300,
        deposit_limit: 0,
        enabled: false,
    };
    let ix = roshi_client::instruction::update_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        asset_pda,
        args,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let mut expected = before;
    expected.custody_token_account = new_custody.to_bytes();
    expected.oracle = new_oracle;
    expected.max_price_change_bps = 300;
    expected.deposit_limit = 0;
    expected.set_enabled(false);
    assert_eq!(load_asset(&svm, asset_pda), expected);
}

#[test]
fn test_update_asset_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let asset_pda = create_asset(&mut svm, &vault, Pubkey::new_unique());
    let before = load_asset(&svm, asset_pda);

    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let ix = roshi_client::instruction::update_asset(
        outsider.pubkey(),
        vault.address,
        asset_pda,
        update_args(Pubkey::new_unique()),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert_eq!(load_asset(&svm, asset_pda), before);
}

#[test]
fn test_update_asset_rejects_invalid_bps() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let asset_pda = create_asset(&mut svm, &vault, Pubkey::new_unique());
    let before = load_asset(&svm, asset_pda);

    let mut args = update_args(Pubkey::new_unique());
    args.max_price_change_bps = 10_001;
    let ix = roshi_client::instruction::update_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        asset_pda,
        args,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidBps,
    );

    assert_eq!(load_asset(&svm, asset_pda), before);
}

#[test]
fn test_update_asset_rejects_foreign_vault() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    // Create the asset under vault A, then try to update it through vault B.
    let vault_a = VaultBuilder::new().install(&mut svm);
    let asset_pda = create_asset(&mut svm, &vault_a, Pubkey::new_unique());
    let before = load_asset(&svm, asset_pda);

    let vault_b = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault_b.roles.admin);
    let ix = roshi_client::instruction::update_asset(
        vault_b.roles.admin.pubkey(),
        vault_b.address,
        asset_pda,
        update_args(Pubkey::new_unique()),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault_b.roles.admin),
        InstructionError::InvalidSeeds,
    );

    assert_eq!(load_asset(&svm, asset_pda), before);
}
