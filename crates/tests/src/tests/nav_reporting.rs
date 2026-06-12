//! Trusted NAV reporting tests. These cover the authority boundary, report
//! commitment storage, fee accrual, and liability-aware gross NAV handling.

use roshi::{error::RoshiError, state::sub_account::VaultSubAccount};
use roshi_interface::instructions::UpdateVaultConfigArgs;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, associated_token_address,
    associated_token_address_with_program, fund, send, send_ok, send_ok_signed, set_ata,
    set_mint_supply, set_token_account, setup_program, token_balance, TestVault, VaultBuilder,
    TOKEN_2022_PROGRAM_ID,
};

const ONE_BASE: u64 = 1_000_000;
const ONE_BASE_SHARES: u64 = 1_000_000_000;

/// The vault's deposit and withdraw sub-account base ATAs — the accounts the
/// program reads idle base from (deposit = index 0, withdraw = index 1: the
/// `VaultBuilder` defaults).
fn base_custodies(vault: &TestVault) -> (Pubkey, Pubkey) {
    let deposit = VaultSubAccount::find_address(&vault.address, 0).0;
    let withdraw = VaultSubAccount::find_address(&vault.address, 1).0;
    (
        associated_token_address(&deposit, &vault.base_mint),
        associated_token_address(&withdraw, &vault.base_mint),
    )
}

/// Report `external_value`; the program reads idle base on-chain from the
/// (passed) deposit and withdraw base ATAs, so gross NAV = idle + external_value.
fn report_nav_ix(
    nav_authority: Pubkey,
    vault: &TestVault,
    external_value: u64,
    report_hash: [u8; 32],
) -> solana_instruction::Instruction {
    let (deposit_custody, withdraw_custody) = base_custodies(vault);
    roshi_client::instruction::report_nav(
        nav_authority,
        vault.address,
        vault.share_mint,
        vault.base_mint,
        deposit_custody,
        withdraw_custody,
        external_value,
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
        crate::helpers::TOKEN_PROGRAM_ID,
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
        &owner.pubkey(),
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
        &vault,
        1_000_000,
        report_hash,
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_000_000);
    assert_eq!(state.last_report_hash, report_hash);
}

#[test]
fn test_report_nav_sums_idle_base_and_external_value() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    // Idle base sitting in the deposit sub-account's base ATA, read on-chain.
    let deposit_sub = VaultSubAccount::find_address(&vault.address, 0).0;
    set_ata(&mut svm, &deposit_sub, &vault.base_mint, 1_000_000);

    // Gross = idle (1_000_000) + external (500_000).
    let ix = report_nav_ix(vault.roles.nav_authority.pubkey(), &vault, 500_000, [1; 32]);
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    assert_eq!(vault.load(&svm).total_assets, 1_500_000);
}

#[test]
fn test_report_nav_counts_withdraw_buffer_as_idle() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    // Base parked in the withdraw sub-account (index 1) buffer is idle too.
    let withdraw_sub = VaultSubAccount::find_address(&vault.address, 1).0;
    set_ata(&mut svm, &withdraw_sub, &vault.base_mint, 250_000);

    let ix = report_nav_ix(vault.roles.nav_authority.pubkey(), &vault, 0, [1; 32]);
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    assert_eq!(vault.load(&svm).total_assets, 250_000);
}

#[test]
fn test_report_nav_rejects_non_canonical_custody() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    // A base account that is not the canonical deposit ATA must be rejected —
    // the authority cannot substitute a sandbagged balance.
    let (_, withdraw_custody) = base_custodies(&vault);
    let ix = roshi_client::instruction::report_nav(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        vault.base_mint,
        Pubkey::new_unique(),
        withdraw_custody,
        0,
        [1; 32],
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        InstructionError::InvalidSeeds,
    );
}

#[test]
fn test_report_nav_rejects_wrong_token_program_ata_namespace() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.nav_authority);

    let deposit_sub = VaultSubAccount::find_address(&vault.address, 0).0;
    let wrong_deposit_custody = associated_token_address_with_program(
        &deposit_sub,
        &vault.base_mint,
        &TOKEN_2022_PROGRAM_ID,
    );
    let (_, withdraw_custody) = base_custodies(&vault);
    let ix = roshi_client::instruction::report_nav(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        vault.base_mint,
        wrong_deposit_custody,
        withdraw_custody,
        0,
        [1; 32],
    )
    .unwrap();

    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        InstructionError::InvalidSeeds,
    );
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

    let ix = report_nav_ix(outsider.pubkey(), &vault, 1_000_000, [1; 32]);
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
        &vault,
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
        &vault,
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
        &vault,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
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
        &vault,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
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
        &vault,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let before = vault.load(&svm);
    let ix = report_nav_ix(vault.roles.nav_authority.pubkey(), &vault, 9_999, [3; 32]);
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        RoshiError::InvalidVaultState,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_report_nav_includes_unstruck_withdrawal_shares_in_fee_denominator() {
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
    assert_eq!(state.total_assets, 1_000_000);
    assert_eq!(state.pending_withdrawal_assets, 0);
    assert_eq!(state.requested_withdrawal_shares, ONE_BASE_SHARES / 2);

    // The deposited base is idle in the deposit sub-account ATA, read on-chain;
    // nothing is deployed externally, so report `external_value = 0`.
    let ix = report_nav_ix(vault.roles.nav_authority.pubkey(), &vault, 0, [1; 32]);
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_000_000);
    assert_eq!(state.pending_withdrawal_assets, 0);
    assert_eq!(state.requested_withdrawal_shares, ONE_BASE_SHARES / 2);
    assert_eq!(state.fees_payable, 0);
    assert_eq!(state.high_watermark, 1_000_000);
    assert_eq!(state.report_epoch, 1);
}

#[test]
fn test_report_nav_rejects_gross_nav_below_payables_with_unstruck_withdrawals() {
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
        &vault,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
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
    assert_eq!(before.pending_withdrawal_assets, 0);
    assert_eq!(before.requested_withdrawal_shares, ONE_BASE_SHARES / 2);

    let ix = report_nav_ix(vault.roles.nav_authority.pubkey(), &vault, 9_999, [3; 32]);
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.nav_authority),
        RoshiError::InvalidVaultState,
    );

    assert_eq!(vault.load(&svm), before);
}

#[test]
fn test_collect_fees_pays_treasury_without_changing_total_assets() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let treasury = Pubkey::new_unique();
    let builder = VaultBuilder::new().treasury(treasury).fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_mint_supply(&mut svm, &vault.share_mint, 1_000_000_000);
    set_token_account(
        &mut svm,
        treasury,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );
    fund(&mut svm, &vault.roles.nav_authority);
    fund(&mut svm, &vault.roles.admin);

    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    // Fees settle out of the vault's deposit custody (an idle sub-account).
    let fee_sub_account_index = 0;
    let fee_sub_account = VaultSubAccount::find_address(&vault.address, fee_sub_account_index).0;
    let custody = set_ata(&mut svm, &fee_sub_account, &vault.base_mint, 10_000);
    let ix = roshi_client::instruction::collect_fees(
        vault.roles.admin.pubkey(),
        vault.address,
        fee_sub_account_index,
        fee_sub_account,
        custody,
        treasury,
        10_000,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_090_000);
    assert_eq!(state.fees_payable, 0);
    assert_eq!(token_balance(&svm, &custody), 0);
    assert_eq!(token_balance(&svm, &treasury), 10_000);
}

/// Flip on external investing through the real config instruction (the
/// builder installs vaults with it disabled).
fn enable_external(svm: &mut litesvm::LiteSVM, vault: &TestVault) {
    let state = vault.load(svm);
    set_token_account(
        svm,
        Pubkey::from(state.treasury),
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        UpdateVaultConfigArgs {
            treasury: state.treasury,
            deposit_sub_account: state.deposit_sub_account,
            withdraw_sub_account: state.withdraw_sub_account,
            base_oracle: state.base_oracle,
            performance_fee_bps: state.performance_fee_bps,
            withdrawal_buffer_bps: state.withdrawal_buffer_bps,
            controls: state.controls,
            external_enabled: true,
        },
    )
    .unwrap();
    send_ok(svm, ix, &vault.roles.admin);
}

#[test]
fn test_invest_external_rejects_unregistered_destination() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);

    let owner = Keypair::new();
    fund(&mut svm, &owner);
    fund(&mut svm, &vault.roles.admin);
    fund(&mut svm, &vault.roles.strategist);

    let share_account = set_ata(&mut svm, &owner.pubkey(), &vault.share_mint, 0);
    deposit_one_base(&mut svm, &vault, &owner, share_account);

    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = associated_token_address(&sub_account, &vault.base_mint);
    let external_account = Pubkey::new_unique();
    set_token_account(
        &mut svm,
        external_account,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );
    enable_external(&mut svm, &vault);

    let (external_destination, _) =
        roshi::state::external_destination::ExternalDestination::find_address(
            &vault.address,
            &external_account,
        );
    let invest = || {
        roshi_client::instruction::invest_external(
            vault.roles.strategist.pubkey(),
            vault.address,
            0,
            sub_account,
            custody,
            external_account,
            external_destination,
            400_000,
        )
        .unwrap()
    };

    // Unregistered destination: the strategist cannot move custody out.
    assert_roshi_error(
        send(&mut svm, invest(), &vault.roles.strategist),
        RoshiError::ExternalDestinationNotRegistered,
    );
    assert_eq!(token_balance(&svm, &external_account), 0);

    // Registered: the same instruction goes through.
    send_ok(
        &mut svm,
        roshi_client::instruction::register_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            external_account,
            external_destination,
        )
        .unwrap(),
        &vault.roles.admin,
    );
    send_ok(&mut svm, invest(), &vault.roles.strategist);
    assert_eq!(token_balance(&svm, &external_account), 400_000);

    // Revoked: closed registration blocks further investment.
    svm.expire_blockhash();
    send_ok(
        &mut svm,
        roshi_client::instruction::revoke_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            external_destination,
        )
        .unwrap(),
        &vault.roles.admin,
    );
    assert_roshi_error(
        send(&mut svm, invest(), &vault.roles.strategist),
        RoshiError::ExternalDestinationNotRegistered,
    );
    assert_eq!(token_balance(&svm, &external_account), 400_000);
}

#[test]
fn test_invest_external_moves_cash_without_changing_total_assets() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);

    let owner = Keypair::new();
    fund(&mut svm, &owner);
    fund(&mut svm, &vault.roles.admin);
    fund(&mut svm, &vault.roles.strategist);

    let share_account = set_ata(&mut svm, &owner.pubkey(), &vault.share_mint, 0);
    deposit_one_base(&mut svm, &vault, &owner, share_account);

    let sub_account_index = 0;
    let sub_account = VaultSubAccount::find_address(&vault.address, sub_account_index).0;
    let custody = associated_token_address(&sub_account, &vault.base_mint);
    let external_authority = Keypair::new();
    fund(&mut svm, &external_authority);
    let external_account = Pubkey::new_unique();
    set_token_account(
        &mut svm,
        external_account,
        &vault.base_mint,
        &external_authority.pubkey(),
        0,
    );
    let state = vault.load(&svm);
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        UpdateVaultConfigArgs {
            treasury: state.treasury,
            deposit_sub_account: state.deposit_sub_account,
            withdraw_sub_account: state.withdraw_sub_account,
            base_oracle: state.base_oracle,
            performance_fee_bps: state.performance_fee_bps,
            withdrawal_buffer_bps: state.withdrawal_buffer_bps,
            controls: state.controls,
            external_enabled: true,
        },
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let (external_destination, _) =
        roshi::state::external_destination::ExternalDestination::find_address(
            &vault.address,
            &external_account,
        );
    send_ok(
        &mut svm,
        roshi_client::instruction::register_external_destination(
            vault.roles.admin.pubkey(),
            vault.address,
            external_account,
            external_destination,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    let ix = roshi_client::instruction::invest_external(
        vault.roles.strategist.pubkey(),
        vault.address,
        sub_account_index,
        sub_account,
        custody,
        external_account,
        external_destination,
        400_000,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.strategist);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, ONE_BASE);
    assert_eq!(state.external_assets, 400_000);
    assert_eq!(token_balance(&svm, &custody), 600_000);
    assert_eq!(token_balance(&svm, &external_account), 400_000);

    let ix = roshi_client::instruction::return_external(
        vault.roles.strategist.pubkey(),
        external_authority.pubkey(),
        vault.address,
        sub_account_index,
        sub_account,
        external_account,
        custody,
        150_000,
    )
    .unwrap();
    send_ok_signed(
        &mut svm,
        ix,
        &vault.roles.strategist,
        &[&external_authority],
    );

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, ONE_BASE);
    assert_eq!(state.external_assets, 250_000);
    assert_eq!(token_balance(&svm, &custody), 750_000);
    assert_eq!(token_balance(&svm, &external_account), 250_000);
}

#[test]
fn test_collect_fees_rejects_amount_above_payable() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let treasury = Pubkey::new_unique();
    let builder = VaultBuilder::new().treasury(treasury);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_token_account(
        &mut svm,
        treasury,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );
    fund(&mut svm, &vault.roles.admin);

    // Fees settle out of the vault's deposit custody (an idle sub-account).
    let fee_sub_account_index = 0;
    let fee_sub_account = VaultSubAccount::find_address(&vault.address, fee_sub_account_index).0;
    let custody = set_ata(&mut svm, &fee_sub_account, &vault.base_mint, 1);
    let ix = roshi_client::instruction::collect_fees(
        vault.roles.admin.pubkey(),
        vault.address,
        fee_sub_account_index,
        fee_sub_account,
        custody,
        treasury,
        1,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidVaultState,
    );
}

#[test]
fn test_collect_fees_rejects_non_idle_sub_account() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let treasury = Pubkey::new_unique();
    let builder = VaultBuilder::new().treasury(treasury).fees(1_000, 250);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_mint_supply(&mut svm, &vault.share_mint, 1_000_000_000);
    fund(&mut svm, &vault.roles.nav_authority);
    fund(&mut svm, &vault.roles.admin);

    // Accrue a payable so collection clears the amount-vs-payable gate and
    // reaches the sub-account check.
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
        1_000_000,
        [1; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);
    let ix = report_nav_ix(
        vault.roles.nav_authority.pubkey(),
        &vault,
        1_100_000,
        [2; 32],
    );
    send_ok(&mut svm, ix, &vault.roles.nav_authority);

    // Index 7 is neither the deposit (0) nor withdraw (1) sub-account, so the
    // program refuses to settle fees out of base it never counts as idle.
    let stray_index = 7;
    let stray_sub_account = VaultSubAccount::find_address(&vault.address, stray_index).0;
    let custody = set_ata(&mut svm, &stray_sub_account, &vault.base_mint, 10_000);
    let ix = roshi_client::instruction::collect_fees(
        vault.roles.admin.pubkey(),
        vault.address,
        stray_index,
        stray_sub_account,
        custody,
        treasury,
        1,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidSubAccount,
    );
}

#[test]
fn test_invest_external_rejects_non_idle_sub_account() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    fund(&mut svm, &vault.roles.strategist);

    let state = vault.load(&svm);
    let ix = roshi_client::instruction::update_vault_config(
        vault.roles.admin.pubkey(),
        vault.address,
        UpdateVaultConfigArgs {
            treasury: state.treasury,
            deposit_sub_account: state.deposit_sub_account,
            withdraw_sub_account: state.withdraw_sub_account,
            base_oracle: state.base_oracle,
            performance_fee_bps: state.performance_fee_bps,
            withdrawal_buffer_bps: state.withdrawal_buffer_bps,
            controls: state.controls,
            external_enabled: true,
        },
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    // Deploying from a sub-account the NAV read never sees would strand base
    // outside idle accounting — rejected before any token movement.
    let stray_index = 7;
    let stray_sub_account = VaultSubAccount::find_address(&vault.address, stray_index).0;
    let custody = associated_token_address(&stray_sub_account, &vault.base_mint);
    let ix = roshi_client::instruction::invest_external(
        vault.roles.strategist.pubkey(),
        vault.address,
        stray_index,
        stray_sub_account,
        custody,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        1,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.strategist),
        RoshiError::InvalidSubAccount,
    );
}

#[test]
fn test_collect_fees_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let treasury = Pubkey::new_unique();
    let builder = VaultBuilder::new().treasury(treasury);
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_token_account(
        &mut svm,
        treasury,
        &vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );

    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    // Fees settle out of the vault's deposit custody (an idle sub-account).
    let fee_sub_account_index = 0;
    let fee_sub_account = VaultSubAccount::find_address(&vault.address, fee_sub_account_index).0;
    let custody = set_ata(&mut svm, &fee_sub_account, &vault.base_mint, 1);
    let ix = roshi_client::instruction::collect_fees(
        outsider.pubkey(),
        vault.address,
        fee_sub_account_index,
        fee_sub_account,
        custody,
        treasury,
        1,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );
}
