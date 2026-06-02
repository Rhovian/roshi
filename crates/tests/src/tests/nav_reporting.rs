//! Trusted NAV reporting tests. These cover the authority boundary, report
//! commitment storage, fee accrual, and liability-aware gross NAV handling.

use roshi::{error::RoshiError, state::sub_account::VaultSubAccount};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, set_ata, set_mint_supply,
    set_token_account, setup_program, token_balance, TestVault, VaultBuilder,
};

const ONE_BASE: u64 = 1_000_000;
const ONE_BASE_SHARES: u64 = 1_000_000_000;

fn report_nav_ix(
    nav_authority: Pubkey,
    vault: Pubkey,
    share_mint: Pubkey,
    total_assets: u64,
    report_hash: [u8; 32],
) -> solana_instruction::Instruction {
    roshi_client::instruction::report_nav(
        nav_authority,
        vault,
        share_mint,
        total_assets,
        report_hash,
    )
    .unwrap()
}

fn deposit_one_base(
    svm: &mut litesvm::LiteSVM,
    vault: &TestVault,
    owner: &Keypair,
    share_account: Pubkey,
) {
    let deposit_sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = set_ata(svm, &deposit_sub_account, &vault.base_mint, 0);
    let source = set_ata(svm, &owner.pubkey(), &vault.base_mint, ONE_BASE);

    let ix = roshi_client::instruction::deposit(
        owner.pubkey(),
        vault.address,
        source,
        custody,
        share_account,
        vault.share_mint,
        vault.base_mint,
        ONE_BASE,
        0,
        vec![],
        vec![],
    )
    .unwrap();
    send_ok(svm, ix, owner);
    svm.expire_blockhash();
}

fn redeem_half_shares(
    svm: &mut litesvm::LiteSVM,
    vault: &TestVault,
    owner: &Keypair,
    share_account: Pubkey,
) {
    let recipient = Pubkey::new_unique();
    set_token_account(svm, recipient, &vault.base_mint, &owner.pubkey(), 0);
    let ticket_index = 0;
    let ticket = roshi::state::withdrawal_ticket::WithdrawalTicket::find_address(
        &vault.address,
        &recipient,
        ticket_index,
    )
    .0;
    let ix = roshi_client::instruction::redeem(
        owner.pubkey(),
        vault.address,
        share_account,
        vault.share_mint,
        recipient,
        ticket,
        ticket_index,
        ONE_BASE_SHARES / 2,
        0,
    )
    .unwrap();
    send_ok(svm, ix, owner);
    svm.expire_blockhash();
}

#[test]
fn test_report_nav_accepts_first_report() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let report_hash = [1; 32];
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
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

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    let before = vault.load(&svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let ix = report_nav_ix(
        outsider.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
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

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    let before = vault.load(&svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
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
fn test_report_nav_accepts_initial_report() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new().fees(100, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_000_000);
    assert_eq!(state.last_report_hash, [1; 32]);
}

#[test]
fn test_report_nav_accrues_performance_fees_against_high_watermark() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new().fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_mint_supply(&mut svm, &vault.share_mint, 1_000_000_000);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_090_000);
    assert_eq!(state.fees_payable, 10_000);
    assert_eq!(state.high_watermark, 1_090_000);
    assert_eq!(state.last_report_hash, [2; 32]);
}

#[test]
fn test_report_nav_excludes_unpaid_fees_from_next_fee_base() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new().fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_mint_supply(&mut svm, &vault.share_mint, 1_000_000_000);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_121_000,
        [3; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_108_900);
    assert_eq!(state.fees_payable, 12_100);
    assert_eq!(state.high_watermark, 1_108_900);
    assert_eq!(state.last_report_hash, [3; 32]);
}

#[test]
fn test_report_nav_rejects_gross_nav_below_existing_payable() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new().fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_mint_supply(&mut svm, &vault.share_mint, 1_000_000_000);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let before = vault.load(&svm);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        9_999,
        [3; 32],
    );
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        RoshiError::InvalidVaultState,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_report_nav_excludes_pending_withdrawals_from_fee_base() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new().fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let owner = Keypair::new();
    fund(&mut svm, &owner);
    let share_account = set_ata(&mut svm, &owner.pubkey(), &vault.share_mint, 0);
    deposit_one_base(&mut svm, &vault, &owner, share_account);
    redeem_half_shares(&mut svm, &vault, &owner, share_account);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 500_000);
    assert_eq!(state.pending_withdrawal_assets, 500_000);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 500_000);
    assert_eq!(state.pending_withdrawal_assets, 500_000);
    assert_eq!(state.fees_payable, 0);
    assert_eq!(state.high_watermark, 1_000_000);
}

#[test]
fn test_report_nav_rejects_gross_nav_below_pending_withdrawals_and_payables() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new().fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_mint_supply(&mut svm, &vault.share_mint, 1_000_000_000);
    fund(&mut svm, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let owner = Keypair::new();
    fund(&mut svm, &owner);
    let share_account = set_ata(&mut svm, &owner.pubkey(), &vault.share_mint, 0);
    set_mint_supply(&mut svm, &vault.share_mint, ONE_BASE_SHARES);
    set_token_account(
        &mut svm,
        share_account,
        &vault.share_mint,
        &owner.pubkey(),
        ONE_BASE_SHARES,
    );
    redeem_half_shares(&mut svm, &vault, &owner, share_account);

    let before = vault.load(&svm);
    assert_eq!(before.fees_payable, 10_000);
    assert_eq!(before.pending_withdrawal_assets, 545_000);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        554_999,
        [3; 32],
    );
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        RoshiError::InvalidVaultState,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_collect_fees_pays_collector_without_changing_total_assets() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fee_collector = Pubkey::new_unique();
    let builder = VaultBuilder::new()
        .fee_collector(fee_collector)
        .fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_mint_supply(&mut svm, &vault.share_mint, 1_000_000_000);
    set_token_account(
        &mut svm,
        fee_collector,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );
    fund(&mut svm, &vault.roles.nav_authority);
    fund(&mut svm, &vault.roles.admin);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let fee_sub_account_index = 7;
    let fee_sub_account = VaultSubAccount::find_address(&vault.address, fee_sub_account_index).0;
    let custody = set_ata(&mut svm, &fee_sub_account, &vault.base_mint, 10_000);
    let ix = roshi_client::instruction::collect_fees(
        vault.roles.admin.pubkey(),
        vault.address,
        fee_sub_account_index,
        fee_sub_account,
        custody,
        fee_collector,
        10_000,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_090_000);
    assert_eq!(state.fees_payable, 0);
    assert_eq!(token_balance(&svm, &custody), 0);
    assert_eq!(token_balance(&svm, &fee_collector), 10_000);
}

#[test]
fn test_collect_fees_rejects_amount_above_payable() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fee_collector = Pubkey::new_unique();
    let builder = VaultBuilder::new().fee_collector(fee_collector);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_token_account(
        &mut svm,
        fee_collector,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );
    fund(&mut svm, &vault.roles.admin);

    let fee_sub_account_index = 7;
    let fee_sub_account = VaultSubAccount::find_address(&vault.address, fee_sub_account_index).0;
    let custody = set_ata(&mut svm, &fee_sub_account, &vault.base_mint, 1);
    let ix = roshi_client::instruction::collect_fees(
        vault.roles.admin.pubkey(),
        vault.address,
        fee_sub_account_index,
        fee_sub_account,
        custody,
        fee_collector,
        1,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidVaultState,
    );
}

#[test]
fn test_collect_fees_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fee_collector = Pubkey::new_unique();
    let builder = VaultBuilder::new().fee_collector(fee_collector);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_token_account(
        &mut svm,
        fee_collector,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );

    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let fee_sub_account_index = 7;
    let fee_sub_account = VaultSubAccount::find_address(&vault.address, fee_sub_account_index).0;
    let custody = set_ata(&mut svm, &fee_sub_account, &vault.base_mint, 1);
    let ix = roshi_client::instruction::collect_fees(
        outsider.pubkey(),
        vault.address,
        fee_sub_account_index,
        fee_sub_account,
        custody,
        fee_collector,
        1,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );
}
