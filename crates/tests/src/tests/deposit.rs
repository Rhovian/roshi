//! `deposit`: pull base/non-base tokens into vault custody and mint shares.
//! litesvm runs the real SPL Token program, so the transfer + mint_to CPIs
//! execute end to end. The share mint authority is the vault PDA.

use roshi::{
    error::RoshiError,
    instructions::InitializeAssetArgs,
    oracle::{OracleConfig, PythOracleConfig, ScopeOracleConfig, ScopeOracleMapping},
    state::{asset::Asset, sub_account::VaultSubAccount},
};
use roshi_interface::access::access_merkle_leaf;
use solana_instruction::AccountMeta;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, associated_token_address,
    associated_token_address_with_program, fund, mint_supply, send, send_ok, set_ata,
    set_ata_with_program, set_clock_timestamp, set_mint, set_pyth_price, set_scope_oracle,
    set_token_2022_mint, set_token_account_with_program, setup_program, token_balance,
    VaultBuilder, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};

/// One whole base unit at 6 decimals.
const ONE_BASE: u64 = 1_000_000;
/// First deposit into an empty vault: ONE_BASE * 10^9 / 10^6.
const ONE_BASE_SHARES: u64 = 1_000_000_000;

/// Install a public vault with fresh base/share mints, the share mint owned by
/// the vault PDA, and the deposit-0 custody ATA. Returns the pieces a base
/// deposit needs.
struct BaseDepositFixture {
    vault: crate::helpers::TestVault,
    base_mint: solana_pubkey::Pubkey,
    share_mint: solana_pubkey::Pubkey,
    custody: solana_pubkey::Pubkey,
}

fn install_base_vault(svm: &mut litesvm::LiteSVM, builder: VaultBuilder) -> BaseDepositFixture {
    let base_mint = solana_pubkey::Pubkey::new_unique();
    let vault = builder.base_mint(base_mint).install(svm);
    let share_mint = vault.share_mint;
    set_mint(svm, share_mint, &vault.address, 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = set_ata(svm, &sub_account, &base_mint, 0);
    BaseDepositFixture {
        vault,
        base_mint,
        share_mint,
        custody,
    }
}

#[allow(clippy::too_many_arguments)]
fn base_deposit_ix(
    fixture: &BaseDepositFixture,
    depositor: &solana_pubkey::Pubkey,
    source: solana_pubkey::Pubkey,
    share_dest: solana_pubkey::Pubkey,
    amount: u64,
    min_shares_out: u64,
    proof: Vec<[u8; 32]>,
) -> solana_instruction::Instruction {
    roshi_client::instruction::deposit(
        *depositor,
        fixture.vault.address,
        source,
        fixture.custody,
        share_dest,
        fixture.share_mint,
        TOKEN_PROGRAM_ID,
        fixture.base_mint,
        amount,
        min_shares_out,
        proof,
        vec![],
    )
    .unwrap()
}

#[test]
fn test_deposit_base_first_deposit_mints_initial_shares() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = install_base_vault(&mut svm, VaultBuilder::new());
    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata(&mut svm, &depositor.pubkey(), &fixture.base_mint, ONE_BASE);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &fixture.share_mint, 0);

    let ix = base_deposit_ix(
        &fixture,
        &depositor.pubkey(),
        source,
        share_dest,
        ONE_BASE,
        0,
        vec![],
    );
    send_ok(&mut svm, ix, &depositor);

    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &fixture.custody), ONE_BASE);
    assert_eq!(token_balance(&svm, &share_dest), ONE_BASE_SHARES);

    let state = fixture.vault.load(&svm);
    assert_eq!(state.total_assets, ONE_BASE);
    assert_eq!(mint_supply(&svm, &fixture.share_mint), ONE_BASE_SHARES);
}

#[test]
fn test_deposit_enforces_vault_cap_atomically() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = install_base_vault(&mut svm, VaultBuilder::new().deposit_cap(ONE_BASE));
    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata(
        &mut svm,
        &depositor.pubkey(),
        &fixture.base_mint,
        ONE_BASE + 1,
    );
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &fixture.share_mint, 0);

    send_ok(
        &mut svm,
        base_deposit_ix(
            &fixture,
            &depositor.pubkey(),
            source,
            share_dest,
            ONE_BASE,
            0,
            vec![],
        ),
        &depositor,
    );
    assert_eq!(fixture.vault.load(&svm).total_assets, ONE_BASE);

    assert_roshi_error(
        send(
            &mut svm,
            base_deposit_ix(
                &fixture,
                &depositor.pubkey(),
                source,
                share_dest,
                1,
                0,
                vec![],
            ),
            &depositor,
        ),
        RoshiError::DepositCapExceeded,
    );

    assert_eq!(fixture.vault.load(&svm).total_assets, ONE_BASE);
    assert_eq!(token_balance(&svm, &source), 1);
    assert_eq!(token_balance(&svm, &fixture.custody), ONE_BASE);
    assert_eq!(token_balance(&svm, &share_dest), ONE_BASE_SHARES);
}

#[test]
fn test_deposit_token_2022_base_mint_mints_classic_shares() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let vault = VaultBuilder::new().base_mint(base_mint).install(&mut svm);
    let share_mint = vault.share_mint;
    set_token_2022_mint(&mut svm, base_mint, &vault.address, 6);
    set_mint(&mut svm, share_mint, &vault.address, 9);

    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody =
        set_ata_with_program(&mut svm, &sub_account, &base_mint, 0, TOKEN_2022_PROGRAM_ID);
    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata_with_program(
        &mut svm,
        &depositor.pubkey(),
        &base_mint,
        ONE_BASE,
        TOKEN_2022_PROGRAM_ID,
    );
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let ix = roshi_client::instruction::deposit(
        depositor.pubkey(),
        vault.address,
        source,
        custody,
        share_dest,
        share_mint,
        TOKEN_2022_PROGRAM_ID,
        base_mint,
        ONE_BASE,
        0,
        vec![],
        vec![],
    )
    .unwrap();
    send_ok(&mut svm, ix, &depositor);

    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), ONE_BASE);
    assert_eq!(token_balance(&svm, &share_dest), ONE_BASE_SHARES);
}

#[test]
fn test_deposit_base_second_deposit_is_proportional() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = install_base_vault(&mut svm, VaultBuilder::new());
    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata(
        &mut svm,
        &depositor.pubkey(),
        &fixture.base_mint,
        2 * ONE_BASE,
    );
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &fixture.share_mint, 0);

    send_ok(
        &mut svm,
        base_deposit_ix(
            &fixture,
            &depositor.pubkey(),
            source,
            share_dest,
            ONE_BASE,
            0,
            vec![],
        ),
        &depositor,
    );
    svm.expire_blockhash();
    send_ok(
        &mut svm,
        base_deposit_ix(
            &fixture,
            &depositor.pubkey(),
            source,
            share_dest,
            ONE_BASE,
            0,
            vec![],
        ),
        &depositor,
    );

    // 1:1 share price, so the second deposit mints the same shares as the first.
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &fixture.custody), 2 * ONE_BASE);
    assert_eq!(token_balance(&svm, &share_dest), 2 * ONE_BASE_SHARES);

    let state = fixture.vault.load(&svm);
    assert_eq!(state.total_assets, 2 * ONE_BASE);
    assert_eq!(mint_supply(&svm, &fixture.share_mint), 2 * ONE_BASE_SHARES);
}

#[test]
fn test_deposit_rejects_when_paused() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = install_base_vault(&mut svm, VaultBuilder::new());
    fund(&mut svm, &fixture.vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::set_pause_flags(
            fixture.vault.roles.admin.pubkey(),
            fixture.vault.address,
            true,
            false,
            false,
        )
        .unwrap(),
        &fixture.vault.roles.admin,
    );

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata(&mut svm, &depositor.pubkey(), &fixture.base_mint, ONE_BASE);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &fixture.share_mint, 0);

    let ix = base_deposit_ix(
        &fixture,
        &depositor.pubkey(),
        source,
        share_dest,
        ONE_BASE,
        0,
        vec![],
    );
    assert_roshi_error(send(&mut svm, ix, &depositor), RoshiError::VaultPaused);
    assert_eq!(token_balance(&svm, &source), ONE_BASE);
}

#[test]
fn test_deposit_private_vault_allows_member_and_rejects_outsider() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let member = Keypair::new();
    let outsider = Keypair::new();
    fund(&mut svm, &member);
    fund(&mut svm, &outsider);

    // Single-leaf access tree rooted at the member, so an empty proof admits it.
    let root = access_merkle_leaf(&member.pubkey());
    let fixture = install_base_vault(&mut svm, VaultBuilder::new().private(true, root));

    let member_source = set_ata(&mut svm, &member.pubkey(), &fixture.base_mint, ONE_BASE);
    let member_shares = set_ata(&mut svm, &member.pubkey(), &fixture.share_mint, 0);
    send_ok(
        &mut svm,
        base_deposit_ix(
            &fixture,
            &member.pubkey(),
            member_source,
            member_shares,
            ONE_BASE,
            0,
            vec![],
        ),
        &member,
    );
    assert_eq!(token_balance(&svm, &member_shares), ONE_BASE_SHARES);

    let outsider_source = set_ata(&mut svm, &outsider.pubkey(), &fixture.base_mint, ONE_BASE);
    let outsider_shares = set_ata(&mut svm, &outsider.pubkey(), &fixture.share_mint, 0);
    let ix = base_deposit_ix(
        &fixture,
        &outsider.pubkey(),
        outsider_source,
        outsider_shares,
        ONE_BASE,
        0,
        vec![],
    );
    assert_roshi_error(
        send(&mut svm, ix, &outsider),
        RoshiError::InvalidAccessProof,
    );
    assert_eq!(token_balance(&svm, &outsider_source), ONE_BASE);
}

#[test]
fn test_deposit_rejects_below_min_shares_out() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = install_base_vault(&mut svm, VaultBuilder::new());
    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata(&mut svm, &depositor.pubkey(), &fixture.base_mint, ONE_BASE);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &fixture.share_mint, 0);

    // Demand more shares than the deposit can mint.
    let ix = base_deposit_ix(
        &fixture,
        &depositor.pubkey(),
        source,
        share_dest,
        ONE_BASE,
        ONE_BASE_SHARES + 1,
        vec![],
    );
    assert_roshi_error(send(&mut svm, ix, &depositor), RoshiError::SlippageExceeded);
    assert_eq!(token_balance(&svm, &source), ONE_BASE);
}

#[test]
fn test_deposit_rejects_wrong_custody_account() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let mut fixture = install_base_vault(&mut svm, VaultBuilder::new());
    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata(&mut svm, &depositor.pubkey(), &fixture.base_mint, ONE_BASE);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &fixture.share_mint, 0);

    // A base-mint token account that is not the deposit sub-account's ATA.
    fixture.custody = set_ata(&mut svm, &depositor.pubkey(), &fixture.base_mint, 0);
    let ix = base_deposit_ix(
        &fixture,
        &depositor.pubkey(),
        source,
        share_dest,
        ONE_BASE,
        0,
        vec![],
    );
    assert_roshi_error(
        send(&mut svm, ix, &depositor),
        RoshiError::InvalidTokenAccount,
    );
}

#[test]
fn test_deposit_non_base_prices_through_pyth_oracle() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let vault = VaultBuilder::new().base_mint(base_mint).install(&mut svm);
    let share_mint = vault.share_mint;
    set_mint(&mut svm, share_mint, &vault.address, 9);

    // Register a non-base asset priced by Pyth at 2.0 base per asset unit.
    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &asset_mint);
    let feed_id = [3u8; 32];
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    fund(&mut svm, &vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle: OracleConfig::pyth(PythOracleConfig::new(feed_id, 8, i64::MAX as u64, 250)),
                asset_decimals: 9,
                enabled: true,
                routed: false,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    // Mock price: 2.0 with exponent -8 at output decimals 8 -> value 2 * 10^8.
    let pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, pyth, feed_id, 200_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000u64; // asset atoms
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let ix = roshi_client::instruction::deposit(
        depositor.pubkey(),
        vault.address,
        source,
        custody,
        share_dest,
        share_mint,
        TOKEN_PROGRAM_ID,
        asset_mint,
        amount,
        0,
        vec![],
        vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(pyth, false),
        ],
    )
    .unwrap();
    send_ok(&mut svm, ix, &depositor);

    // 2.0 base per whole asset token, scaled across 9 asset / 6 base decimals:
    // base_atoms = amount * 2 * 10^(6-9). First deposit -> base_atoms * 10^9 /
    // 10^6 shares.
    let base_atoms = amount * 2 / 1_000;
    let shares = base_atoms * 1_000;
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), amount);
    assert_eq!(token_balance(&svm, &share_dest), shares);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, base_atoms);
    assert_eq!(mint_supply(&svm, &share_mint), shares);
}

/// Install a vault plus a non-base asset priced through a mock Scope oracle at
/// 2.0 base per whole asset token (value 2 * 10^17 at exp 17), with the entry's
/// observation `age_seconds` behind the cluster clock. Returns everything the
/// deposit instruction needs.
fn install_scope_priced_asset(
    svm: &mut litesvm::LiteSVM,
    age_seconds: u64,
    routed: bool,
    base_oracle: OracleConfig,
) -> (
    crate::helpers::TestVault,
    solana_pubkey::Pubkey, // asset mint
    solana_pubkey::Pubkey, // asset pda
    solana_pubkey::Pubkey, // custody
    solana_pubkey::Pubkey, // scope prices account
    solana_pubkey::Pubkey, // scope mappings account
) {
    const NOW: i64 = 1_787_619_100;

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let vault = VaultBuilder::new()
        .base_mint(base_mint)
        .base_oracle(base_oracle)
        .install(svm);
    set_mint(svm, vault.share_mint, &vault.address, 9);
    set_clock_timestamp(svm, NOW);

    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_mint(svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &asset_mint);
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    let prices_account = solana_pubkey::Pubkey::new_unique();
    let mappings_account = solana_pubkey::Pubkey::new_unique();
    let scope_config = ScopeOracleConfig::new(
        prices_account.to_bytes(),
        ScopeOracleMapping::new([7u8; 32], 26, 9, 3, 17, [8u8; 20]),
        445,
        300,
    );

    fund(svm, &vault.roles.admin);
    send_ok(
        svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle: OracleConfig::scope(scope_config),
                asset_decimals: 9,
                enabled: true,
                routed,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    set_scope_oracle(
        svm,
        &scope_config,
        mappings_account,
        200_000_000_000_000_000, // 2.0 at exp 17
        17,
        NOW as u64 - age_seconds,
    );

    (
        vault,
        asset_mint,
        asset_pda,
        custody,
        prices_account,
        mappings_account,
    )
}

#[test]
fn test_deposit_non_base_prices_through_scope_oracle() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let (vault, asset_mint, asset_pda, custody, prices_account, mappings_account) =
        install_scope_priced_asset(&mut svm, 30, false, OracleConfig::default());
    let share_mint = vault.share_mint;
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000u64; // asset atoms
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let ix = roshi_client::instruction::deposit(
        depositor.pubkey(),
        vault.address,
        source,
        custody,
        share_dest,
        share_mint,
        TOKEN_PROGRAM_ID,
        asset_mint,
        amount,
        0,
        vec![],
        vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(prices_account, false),
            AccountMeta::new_readonly(mappings_account, false),
        ],
    )
    .unwrap();
    send_ok(&mut svm, ix, &depositor);

    // 2.0 base per whole asset token, scaled across 9 asset / 6 base decimals:
    // base_atoms = amount * 2 * 10^17 * 10^6 / 10^(9+17) = amount * 2 / 10^3.
    let base_atoms = amount * 2 / 1_000;
    let shares = base_atoms * 1_000;
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), amount);
    assert_eq!(token_balance(&svm, &share_dest), shares);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, base_atoms);
    assert_eq!(mint_supply(&svm, &share_mint), shares);
}

#[test]
fn test_deposit_routed_scope_asset_advances_to_pyth_base_leg() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_feed_id = [11u8; 32];
    let base_oracle =
        OracleConfig::pyth(PythOracleConfig::new(base_feed_id, 8, i64::MAX as u64, 250));
    let (vault, asset_mint, asset_pda, custody, prices_account, mappings_account) =
        install_scope_priced_asset(&mut svm, 30, true, base_oracle);
    let base_pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, base_pyth, base_feed_id, 100_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000_000u64;
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &vault.share_mint, 0);

    let deposit_via = |oracle_accounts: Vec<AccountMeta>| {
        roshi_client::instruction::deposit(
            depositor.pubkey(),
            vault.address,
            source,
            custody,
            share_dest,
            vault.share_mint,
            TOKEN_PROGRAM_ID,
            asset_mint,
            amount,
            0,
            vec![],
            oracle_accounts,
        )
        .unwrap()
    };

    assert_instruction_error(
        send(
            &mut svm,
            deposit_via(vec![
                AccountMeta::new_readonly(asset_pda, false),
                AccountMeta::new_readonly(prices_account, false),
                AccountMeta::new_readonly(mappings_account, false),
            ]),
            &depositor,
        ),
        crate::helpers::NOT_ENOUGH_ACCOUNT_KEYS,
    );
    assert_eq!(token_balance(&svm, &source), amount);

    send_ok(
        &mut svm,
        deposit_via(vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(prices_account, false),
            AccountMeta::new_readonly(mappings_account, false),
            AccountMeta::new_readonly(base_pyth, false),
        ]),
        &depositor,
    );

    let base_atoms = 2_000_000u64;
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), amount);
    assert_eq!(token_balance(&svm, &share_dest), base_atoms * 1_000);
    assert_eq!(vault.load(&svm).total_assets, base_atoms);
}

#[test]
fn test_deposit_rejects_stale_scope_observation() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    // Observation older than the configured 300 s max age.
    let (vault, asset_mint, asset_pda, custody, prices_account, mappings_account) =
        install_scope_priced_asset(&mut svm, 301, false, OracleConfig::default());
    let share_mint = vault.share_mint;
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000u64;
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let ix = roshi_client::instruction::deposit(
        depositor.pubkey(),
        vault.address,
        source,
        custody,
        share_dest,
        share_mint,
        TOKEN_PROGRAM_ID,
        asset_mint,
        amount,
        0,
        vec![],
        vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(prices_account, false),
            AccountMeta::new_readonly(mappings_account, false),
        ],
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &depositor),
        solana_sdk::instruction::InstructionError::InvalidAccountData,
    );
}

#[test]
fn test_deposit_rejects_above_asset_deposit_cap() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let vault = VaultBuilder::new().base_mint(base_mint).install(&mut svm);
    let share_mint = vault.share_mint;
    set_mint(&mut svm, share_mint, &vault.address, 9);

    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &asset_mint);
    let feed_id = [3u8; 32];
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    fund(&mut svm, &vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle: OracleConfig::pyth(PythOracleConfig::new(feed_id, 8, i64::MAX as u64, 250)),
                asset_decimals: 9,
                enabled: true,
                routed: false,
                deposit_cap_atoms: 1_500_000,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    let pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, pyth, feed_id, 200_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, 1_900_000);
    // The cap is an inventory cap: pre-existing custody balance counts.
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 600_000);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let deposit_ix = |amount: u64| {
        roshi_client::instruction::deposit(
            depositor.pubkey(),
            vault.address,
            source,
            custody,
            share_dest,
            share_mint,
            TOKEN_PROGRAM_ID,
            asset_mint,
            amount,
            0,
            vec![],
            vec![
                AccountMeta::new_readonly(asset_pda, false),
                AccountMeta::new_readonly(pyth, false),
            ],
        )
        .unwrap()
    };

    // 600_000 + 1_000_000 exceeds the 1_500_000 cap; nothing moves.
    assert_roshi_error(
        send(&mut svm, deposit_ix(1_000_000), &depositor),
        RoshiError::DepositCapExceeded,
    );
    assert_eq!(token_balance(&svm, &custody), 600_000);
    assert_eq!(token_balance(&svm, &share_dest), 0);

    // Filling custody exactly to the cap is allowed.
    send_ok(&mut svm, deposit_ix(900_000), &depositor);
    assert_eq!(token_balance(&svm, &custody), 1_500_000);
}

#[test]
fn test_deposit_routed_asset_composes_asset_and_base_oracle_legs() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    // USDC-like base (6 decimals) priced in USD by the vault's base oracle;
    // SOL-like asset (9 decimals) priced in USD by its own feed. Routed
    // pricing composes asset/USD over base/USD.
    let base_mint = solana_pubkey::Pubkey::new_unique();
    let base_feed_id = [11u8; 32];
    let vault = VaultBuilder::new()
        .base_mint(base_mint)
        .base_oracle(OracleConfig::pyth(PythOracleConfig::new(
            base_feed_id,
            8,
            i64::MAX as u64,
            250,
        )))
        .install(&mut svm);
    let share_mint = vault.share_mint;
    set_mint(&mut svm, share_mint, &vault.address, 9);

    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &asset_mint);
    let asset_feed_id = [12u8; 32];
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    fund(&mut svm, &vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle: OracleConfig::pyth(PythOracleConfig::new(
                    asset_feed_id,
                    8,
                    i64::MAX as u64,
                    250,
                )),
                asset_decimals: 9,
                enabled: true,
                routed: true,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    // Asset/USD at 150.0 and base/USD at 1.0, both exponent -8.
    let asset_pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, asset_pyth, asset_feed_id, 15_000_000_000, -8, 0);
    let base_pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, base_pyth, base_feed_id, 100_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000_000u64; // one whole 9-decimal asset token
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let deposit_via = |oracle_accounts: Vec<AccountMeta>| {
        roshi_client::instruction::deposit(
            depositor.pubkey(),
            vault.address,
            source,
            custody,
            share_dest,
            share_mint,
            TOKEN_PROGRAM_ID,
            asset_mint,
            amount,
            0,
            vec![],
            oracle_accounts,
        )
        .unwrap()
    };

    // A routed deposit without the base-oracle leg fails closed.
    assert_instruction_error(
        send(
            &mut svm,
            deposit_via(vec![
                AccountMeta::new_readonly(asset_pda, false),
                AccountMeta::new_readonly(asset_pyth, false),
            ]),
            &depositor,
        ),
        crate::helpers::NOT_ENOUGH_ACCOUNT_KEYS,
    );
    assert_eq!(token_balance(&svm, &source), amount);

    send_ok(
        &mut svm,
        deposit_via(vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(asset_pyth, false),
            AccountMeta::new_readonly(base_pyth, false),
        ]),
        &depositor,
    );

    // 1 asset token at 150 USD over 1 USD base: 150 whole base = 150_000_000
    // base atoms, scaled 1000x into shares.
    let base_atoms = 150_000_000u64;
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), amount);
    assert_eq!(token_balance(&svm, &share_dest), base_atoms * 1_000);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, base_atoms);
}

/// A routed asset whose oracle is identical to the vault base oracle prices at
/// ratio 1 from a single verified update: the base leg's accounts stay in the
/// layout but are never consulted, so a depositor cannot pair a high asset-leg
/// update with a lower still-fresh update of the same feed.
#[test]
fn test_deposit_routed_asset_dedups_against_base_feed() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let feed_id = [11u8; 32];
    let oracle = OracleConfig::pyth(PythOracleConfig::new(feed_id, 8, i64::MAX as u64, 250));
    let vault = VaultBuilder::new()
        .base_mint(base_mint)
        .base_oracle(oracle)
        .install(&mut svm);
    let share_mint = vault.share_mint;
    set_mint(&mut svm, share_mint, &vault.address, 9);

    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &asset_mint);
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    fund(&mut svm, &vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle,
                asset_decimals: 9,
                enabled: true,
                routed: true,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    // Two verified updates of the one feed disagreeing 2x: the asset leg at
    // 2.0 and a still-fresh base-leg candidate at 1.0.
    let asset_pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, asset_pyth, feed_id, 200_000_000, -8, 0);
    let cheaper_pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, cheaper_pyth, feed_id, 100_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000_000u64; // one whole 9-decimal asset token
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let deposit_via = |oracle_accounts: Vec<AccountMeta>| {
        roshi_client::instruction::deposit(
            depositor.pubkey(),
            vault.address,
            source,
            custody,
            share_dest,
            share_mint,
            TOKEN_PROGRAM_ID,
            asset_mint,
            amount,
            0,
            vec![],
            oracle_accounts,
        )
        .unwrap()
    };

    // The base leg stays part of the layout even though it is not consulted.
    assert_instruction_error(
        send(
            &mut svm,
            deposit_via(vec![
                AccountMeta::new_readonly(asset_pda, false),
                AccountMeta::new_readonly(asset_pyth, false),
            ]),
            &depositor,
        ),
        crate::helpers::NOT_ENOUGH_ACCOUNT_KEYS,
    );
    assert_eq!(token_balance(&svm, &source), amount);

    send_ok(
        &mut svm,
        deposit_via(vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(asset_pyth, false),
            AccountMeta::new_readonly(cheaper_pyth, false),
        ]),
        &depositor,
    );

    // Ratio 1 from the single asset-leg read: one whole asset token values as
    // one whole base token (1_000_000 six-decimal base atoms). Pricing the
    // legs from the two disagreeing updates would have minted 2x.
    let base_atoms = 1_000_000u64;
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), amount);
    assert_eq!(token_balance(&svm, &share_dest), base_atoms * 1_000);
    assert_eq!(vault.load(&svm).total_assets, base_atoms);
}

/// The same feed under two validation policies is one feed read two ways, not
/// two feeds; a routed deposit configured that way fails closed.
#[test]
fn test_deposit_routed_asset_same_feed_policy_mismatch_rejected() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let feed_id = [11u8; 32];
    let vault = VaultBuilder::new()
        .base_mint(base_mint)
        .base_oracle(OracleConfig::pyth(PythOracleConfig::new(
            feed_id,
            8,
            i64::MAX as u64,
            250,
        )))
        .install(&mut svm);
    let share_mint = vault.share_mint;
    set_mint(&mut svm, share_mint, &vault.address, 9);

    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &asset_mint);
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    fund(&mut svm, &vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                // Same feed as the base oracle but a different max age.
                oracle: OracleConfig::pyth(PythOracleConfig::new(feed_id, 8, 3_600, 250)),
                asset_decimals: 9,
                enabled: true,
                routed: true,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    let pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, pyth, feed_id, 200_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000_000u64;
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let ix = roshi_client::instruction::deposit(
        depositor.pubkey(),
        vault.address,
        source,
        custody,
        share_dest,
        share_mint,
        TOKEN_PROGRAM_ID,
        asset_mint,
        amount,
        0,
        vec![],
        vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(pyth, false),
            AccountMeta::new_readonly(pyth, false),
        ],
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &depositor),
        solana_instruction::error::InstructionError::InvalidAccountData,
    );
    assert_eq!(token_balance(&svm, &source), amount);
    assert_eq!(token_balance(&svm, &share_dest), 0);
}

#[test]
fn test_deposit_pyth_pinned_price_account_rejects_substitutes() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let vault = VaultBuilder::new().base_mint(base_mint).install(&mut svm);
    let share_mint = vault.share_mint;
    set_mint(&mut svm, share_mint, &vault.address, 9);

    // Register a non-base asset whose oracle config pins one specific Pyth
    // price update account.
    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &asset_mint);
    let feed_id = [5u8; 32];
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);
    let pinned_pyth = solana_pubkey::Pubkey::new_unique();

    fund(&mut svm, &vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle: OracleConfig::pyth(
                    PythOracleConfig::new(feed_id, 8, i64::MAX as u64, 250)
                        .pin_price_update_account(pinned_pyth.to_bytes()),
                ),
                asset_decimals: 9,
                enabled: true,
                routed: false,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    // Two equally valid Pyth updates for the configured feed.
    set_pyth_price(&mut svm, pinned_pyth, feed_id, 200_000_000, -8, 0);
    let substitute_pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, substitute_pyth, feed_id, 200_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000u64;
    let source = set_ata(&mut svm, &depositor.pubkey(), &asset_mint, amount);
    crate::helpers::set_token_account(&mut svm, custody, &asset_mint, &sub_account, 0);
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let deposit_via = |price_account: solana_pubkey::Pubkey| {
        roshi_client::instruction::deposit(
            depositor.pubkey(),
            vault.address,
            source,
            custody,
            share_dest,
            share_mint,
            TOKEN_PROGRAM_ID,
            asset_mint,
            amount,
            0,
            vec![],
            vec![
                AccountMeta::new_readonly(asset_pda, false),
                AccountMeta::new_readonly(price_account, false),
            ],
        )
        .unwrap()
    };

    // A verified update for the right feed under a different address fails...
    assert_instruction_error(
        send(&mut svm, deposit_via(substitute_pyth), &depositor),
        solana_instruction::error::InstructionError::InvalidAccountData,
    );
    assert_eq!(token_balance(&svm, &source), amount);

    // ...and the pinned account prices normally: 2.0 base per whole asset
    // token across 9 asset / 6 base decimals, then base atoms scale 1000x
    // into shares.
    send_ok(&mut svm, deposit_via(pinned_pyth), &depositor);
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &share_dest), amount * 2);
}

#[test]
fn test_deposit_mixed_classic_base_token_2022_registered_asset() {
    let Some((mut svm, _authority, _config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let vault = VaultBuilder::new().base_mint(base_mint).install(&mut svm);
    let share_mint = vault.share_mint;
    set_mint(&mut svm, share_mint, &vault.address, 9);

    let asset_mint = solana_pubkey::Pubkey::new_unique();
    set_token_2022_mint(&mut svm, asset_mint, &vault.roles.admin.pubkey(), 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody =
        associated_token_address_with_program(&sub_account, &asset_mint, &TOKEN_2022_PROGRAM_ID);
    let feed_id = [4u8; 32];
    let (asset_pda, _) = Asset::find_address(&vault.address, &asset_mint);

    fund(&mut svm, &vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::initialize_asset(
            vault.roles.admin.pubkey(),
            vault.address,
            asset_mint,
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle: OracleConfig::pyth(PythOracleConfig::new(feed_id, 8, i64::MAX as u64, 250)),
                asset_decimals: 9,
                enabled: true,
                routed: false,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap(),
        &vault.roles.admin,
    );

    let pyth = solana_pubkey::Pubkey::new_unique();
    set_pyth_price(&mut svm, pyth, feed_id, 200_000_000, -8, 0);

    let depositor = Keypair::new();
    fund(&mut svm, &depositor);
    let amount = 1_000_000u64;
    let source = set_ata_with_program(
        &mut svm,
        &depositor.pubkey(),
        &asset_mint,
        amount,
        TOKEN_2022_PROGRAM_ID,
    );
    set_token_account_with_program(
        &mut svm,
        custody,
        &asset_mint,
        &sub_account,
        0,
        TOKEN_2022_PROGRAM_ID,
    );
    let share_dest = set_ata(&mut svm, &depositor.pubkey(), &share_mint, 0);

    let ix = roshi_client::instruction::deposit(
        depositor.pubkey(),
        vault.address,
        source,
        custody,
        share_dest,
        share_mint,
        TOKEN_2022_PROGRAM_ID,
        asset_mint,
        amount,
        0,
        vec![],
        vec![
            AccountMeta::new_readonly(asset_pda, false),
            AccountMeta::new_readonly(pyth, false),
        ],
    )
    .unwrap();
    send_ok(&mut svm, ix, &depositor);

    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), amount);
    // 2.0 base per whole asset token across 9 asset / 6 base decimals, then
    // base atoms scale 1000x into shares.
    assert_eq!(token_balance(&svm, &share_dest), amount * 2);
}
