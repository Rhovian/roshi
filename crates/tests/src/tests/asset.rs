//! `initialize_asset` / `update_asset` manage the per-vault config for a
//! supported non-base deposit asset (oracle, decimals, enabled).
//! Both are admin-gated; the oracle is stored as config and validated on-chain
//! at deposit time, while custody is the derived `ATA(deposit_sub_account,
//! asset_mint)`. `initialize_asset` creates the Asset PDA, `update_asset`
//! mutates its non-immutable fields.

use litesvm::LiteSVM;
use roshi::{
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
    assert_instruction_error, assert_roshi_error, fund, send, send_ok,
    set_extended_token_2022_mint, set_mint, set_token_2022_mint, setup_program, TestVault,
    VaultBuilder,
};

fn init_args(asset_mint: Pubkey) -> InitializeAssetArgs {
    InitializeAssetArgs {
        asset_mint: asset_mint.to_bytes(),
        oracle: OracleConfig::default(),
        asset_decimals: 9,
        enabled: true,
        routed: false,
        deposit_cap_atoms: u64::MAX,
    }
}

fn update_args() -> UpdateAssetArgs {
    UpdateAssetArgs {
        oracle: OracleConfig::default(),
        enabled: false,
        routed: false,
        deposit_cap_atoms: u64::MAX,
    }
}

/// Initialize an enabled asset under `vault` and return its PDA.
fn create_asset(svm: &mut LiteSVM, vault: &TestVault, asset_mint: Pubkey) -> Pubkey {
    fund(svm, &vault.roles.admin);
    set_mint(svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);
    send_ok(
        svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            init_args(asset_mint),
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
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let (asset_pda, bump) = Asset::find_address(&vault.address, &asset_mint);
    assert!(svm.get_account(&asset_pda).is_none());

    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            init_args(asset_mint),
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
    assert_eq!(asset.asset_decimals, 9);
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
        asset_mint,
        asset_pda,
        init_args(asset_mint),
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
        base_mint,
        asset_pda,
        init_args(base_mint),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::InvalidArgument,
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
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    // The asset account does not match the PDA for asset_mint.
    let wrong_asset = Pubkey::new_unique();
    let ix = roshi_client::instruction::initialize_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        asset_mint,
        wrong_asset,
        init_args(asset_mint),
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
        asset_mint,
        asset_pda,
        init_args(asset_mint),
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
    let new_oracle = OracleConfig::pyth(PythOracleConfig::new([4; 32], 8, 30, 250));
    assert_ne!(new_oracle, before.oracle);
    let args = UpdateAssetArgs {
        oracle: new_oracle,
        enabled: false,
        routed: false,
        deposit_cap_atoms: u64::MAX,
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
    expected.oracle = new_oracle;
    expected.set_enabled(false);
    assert_eq!(load_asset(&svm, asset_pda), expected);
}

#[test]
fn test_initialize_asset_accepts_bare_token_2022_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let asset_mint = Pubkey::new_unique();
    set_token_2022_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            init_args(asset_mint),
        )
        .unwrap(),
        &vault.roles.admin,
    );

    let asset = load_asset(&svm, asset_pda);
    assert_eq!(asset.asset_mint, asset_mint.to_bytes());
}

#[test]
fn test_initialize_asset_rejects_extended_token_2022_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let asset_mint = Pubkey::new_unique();
    set_extended_token_2022_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    let ix = roshi_client::instruction::initialize_asset(
        vault.roles.admin.pubkey(),
        vault.address,
        asset_mint,
        asset_pda,
        init_args(asset_mint),
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        roshi::error::RoshiError::InvalidMintAccount,
    );
    assert!(svm.get_account(&asset_pda).is_none());
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
        update_args(),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
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
        update_args(),
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault_b.roles.admin),
        InstructionError::InvalidSeeds,
    );

    assert_eq!(load_asset(&svm, asset_pda), before);
}
