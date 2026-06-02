//! `deposit`: pull base/non-base tokens into vault custody and mint shares.
//! litesvm runs the real SPL Token program, so the transfer + mint_to CPIs
//! execute end to end. The share mint authority is the vault PDA.

use roshi::{
    error::RoshiError,
    instructions::InitializeAssetArgs,
    oracle::{OracleConfig, PythOracleConfig},
    state::{asset::Asset, sub_account::VaultSubAccount},
};
use roshi_interface::access::access_merkle_leaf;
use solana_instruction::AccountMeta;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_roshi_error, associated_token_address, fund, send, send_ok, set_ata, set_mint,
    set_pyth_price, setup_program, token_balance, VaultBuilder,
};

/// One whole base unit at 6 decimals.
const ONE_BASE: u64 = 1_000_000;
/// `initial_shares_from_base_atoms(ONE_BASE, 6)` = ONE_BASE * 10^9 / 10^6.
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
    let share_mint = solana_pubkey::Pubkey::new_unique();
    let vault = builder
        .base_mint(base_mint)
        .share_mint(share_mint)
        .install(svm);
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
    assert_eq!(state.total_shares, ONE_BASE_SHARES);
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
    assert_eq!(state.total_shares, 2 * ONE_BASE_SHARES);
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
    let share_mint = solana_pubkey::Pubkey::new_unique();
    let vault = VaultBuilder::new()
        .base_mint(base_mint)
        .share_mint(share_mint)
        .install(&mut svm);
    set_mint(&mut svm, share_mint, &vault.address, 9);

    // Register a non-base asset priced by Pyth at 2.0 base per asset unit.
    let asset_mint = solana_pubkey::Pubkey::new_unique();
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
            asset_pda,
            InitializeAssetArgs {
                asset_mint: asset_mint.to_bytes(),
                oracle: OracleConfig::pyth(PythOracleConfig::new(feed_id, 8, i64::MAX as u64, 0)),
                asset_decimals: 9,
                enabled: true,
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

    // base_atoms = amount * 2; first deposit -> base_atoms * 10^9 / 10^6 shares.
    let base_atoms = amount * 2;
    let shares = base_atoms * 1_000;
    assert_eq!(token_balance(&svm, &source), 0);
    assert_eq!(token_balance(&svm, &custody), amount);
    assert_eq!(token_balance(&svm, &share_dest), shares);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, base_atoms);
    assert_eq!(state.total_shares, shares);
}
