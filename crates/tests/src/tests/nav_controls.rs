//! Economic NAV controls around `report_nav` and the pricing paths: profit
//! unlock (gains drip over the span they were earned in, losses recognize
//! instantly), the report rate limit, the upward NAV move bound, and the
//! staleness posture (deposits and queued redeems are never gated).

use litesvm::LiteSVM;
use roshi::{error::RoshiError, state::sub_account::VaultSubAccount};
use roshi_interface::{math::shares_for_deposit, state::VaultControls};
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_roshi_error, associated_token_address, fund, send, send_ok, set_ata,
    set_clock_timestamp, set_token_account, setup_program, token_balance, TestVault, VaultBuilder,
};

const ONE_BASE: u64 = 1_000_000;
const ONE_BASE_SHARES: u64 = 1_000_000_000;

/// Install a zero-perf-fee vault with `controls` plus its (empty) deposit
/// custody, and fund the NAV authority.
fn setup_vault(svm: &mut LiteSVM, controls: VaultControls) -> (TestVault, Pubkey) {
    let builder = VaultBuilder::new().controls(controls).fees(0, 250);
    builder.install_mints(svm);
    let vault = builder.install(svm);
    fund(svm, &vault.roles.nav_authority);

    let deposit_sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = set_ata(svm, &deposit_sub_account, &vault.base_mint, 0);
    (vault, custody)
}

/// Deposit `amount` base from a fresh owner; returns the owner and their
/// share account.
fn deposit_base(
    svm: &mut LiteSVM,
    vault: &TestVault,
    custody: Pubkey,
    amount: u64,
) -> (Keypair, Pubkey) {
    let owner = Keypair::new();
    fund(svm, &owner);
    let source = set_ata(svm, &owner.pubkey(), &vault.base_mint, amount);
    let share_account = set_ata(svm, &owner.pubkey(), &vault.share_mint, 0);

    send_ok(
        svm,
        roshi_client::instruction::deposit(
            owner.pubkey(),
            vault.address,
            source,
            custody,
            share_account,
            vault.share_mint,
            crate::helpers::TOKEN_PROGRAM_ID,
            vault.base_mint,
            amount,
            0,
            vec![],
            vec![],
        )
        .unwrap(),
        &owner,
    );
    svm.expire_blockhash();
    (owner, share_account)
}

fn report_nav_ix(
    vault: &TestVault,
    external_value: u64,
    report_hash: [u8; 32],
) -> solana_instruction::Instruction {
    let deposit_sub = VaultSubAccount::find_address(&vault.address, 0).0;
    let withdraw_sub = VaultSubAccount::find_address(&vault.address, 1).0;
    roshi_client::instruction::report_nav(
        vault.roles.nav_authority.pubkey(),
        vault.address,
        vault.share_mint,
        vault.base_mint,
        associated_token_address(&deposit_sub, &vault.base_mint),
        associated_token_address(&withdraw_sub, &vault.base_mint),
        external_value,
        report_hash,
    )
    .unwrap()
}

fn report(svm: &mut LiteSVM, vault: &TestVault, external_value: u64, hash_byte: u8) {
    send_ok(
        svm,
        report_nav_ix(vault, external_value, [hash_byte; 32]),
        &vault.roles.nav_authority,
    );
    svm.expire_blockhash();
}

fn unlock_controls(max_unlock_duration_secs: u32) -> VaultControls {
    VaultControls::new(max_unlock_duration_secs, 0, 0, 0, 0, 0, 0)
}

#[test]
fn test_gain_report_locks_profit_and_prices_deposits_at_effective_nav() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, custody) = setup_vault(&mut svm, unlock_controls(1_000));
    deposit_base(&mut svm, &vault, custody, ONE_BASE);

    // Gross = 1_000_000 idle + 200_000 external; no perf fee configured.
    report(&mut svm, &vault, 200_000, 1);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_200_000);
    assert_eq!(state.locked_profit, 200_000);
    assert_eq!(state.profit_unlock_start_ts, 10_000);
    // Elapsed-since-last-report exceeds the clamp: window = 1_000s.
    assert_eq!(state.profit_unlock_end_ts, 11_000);
    assert_eq!(state.last_update_ts, 10_000);
    // The gain is locked at report time: effective NAV is continuous.
    assert_eq!(state.effective_total_assets(10_000), Ok(1_000_000));
    assert_eq!(state.effective_total_assets(10_500), Ok(1_100_000));

    // A mid-drip deposit prices at effective NAV: the depositor mints as if
    // the still-locked profit does not exist yet, and earns it only as it
    // drips — the same rate as every other holder.
    set_clock_timestamp(&mut svm, 10_500);
    let (_, share_account) = deposit_base(&mut svm, &vault, custody, ONE_BASE);
    let expected = shares_for_deposit(ONE_BASE, 1_100_000, ONE_BASE_SHARES, 6).unwrap();
    assert_eq!(token_balance(&svm, &share_account), expected);
}

#[test]
fn test_second_gain_report_rolls_unfinished_drip_forward() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, custody) = setup_vault(&mut svm, unlock_controls(1_000));
    deposit_base(&mut svm, &vault, custody, ONE_BASE);
    report(&mut svm, &vault, 200_000, 1);

    // Mid-drip: remaining locked 100_000, effective 1_100_000.
    set_clock_timestamp(&mut svm, 10_500);
    report(&mut svm, &vault, 350_000, 2);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_350_000);
    // gain = 1_350_000 - 1_100_000: the unfinished 100_000 re-locks with the
    // new 250_000, over the 500s span it was earned in (below the clamp).
    assert_eq!(state.locked_profit, 250_000);
    assert_eq!(state.profit_unlock_start_ts, 10_500);
    assert_eq!(state.profit_unlock_end_ts, 11_000);
    assert_eq!(state.effective_total_assets(10_500), Ok(1_100_000));
}

#[test]
fn test_loss_report_recognizes_instantly_and_clears_the_drip() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, custody) = setup_vault(&mut svm, unlock_controls(1_000));
    deposit_base(&mut svm, &vault, custody, ONE_BASE);
    report(&mut svm, &vault, 200_000, 1);

    // Mid-drip effective is 1_100_000; reporting gross 1_000_000 is a loss.
    set_clock_timestamp(&mut svm, 10_500);
    report(&mut svm, &vault, 0, 2);

    let state = vault.load(&svm);
    assert_eq!(state.total_assets, 1_000_000);
    assert_eq!(state.locked_profit, 0);
    // The loss landed in full, absorbed by the locked remainder first; the
    // effective NAV never jumped up.
    assert_eq!(state.effective_total_assets(10_500), Ok(1_000_000));
}

#[test]
fn test_deposits_and_queued_redeems_are_never_staleness_gated() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, custody) = setup_vault(&mut svm, VaultControls::new(0, 100, 0, 0, 0, 0, 0));
    let (owner, share_account) = deposit_base(&mut svm, &vault, custody, ONE_BASE);
    report(&mut svm, &vault, 0, 1);

    // Far past max_report_age_secs: deposits stay open (user decision —
    // stale-entry capture is bounded by the drip and the gain bound, and
    // stale-high entry harms only the depositor, who has min_shares_out).
    set_clock_timestamp(&mut svm, 50_000);
    deposit_base(&mut svm, &vault, custody, ONE_BASE);

    // Queued redeems stay open too: they price later, at strike.
    let recipient = Pubkey::new_unique();
    set_token_account(&mut svm, recipient, &vault.base_mint, &owner.pubkey(), 0);
    let ticket = roshi::state::withdrawal_ticket::WithdrawalTicket::find_address(
        &vault.address,
        &owner.pubkey(),
        0,
    )
    .0;
    send_ok(
        &mut svm,
        roshi_client::instruction::redeem(
            owner.pubkey(),
            vault.address,
            share_account,
            vault.share_mint,
            recipient,
            ticket,
            0,
            ONE_BASE_SHARES / 2,
        )
        .unwrap(),
        &owner,
    );
}

#[test]
fn test_report_rejects_gain_above_bound_but_not_losses() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, custody) = setup_vault(&mut svm, VaultControls::new(0, 0, 0, 0, 1_000, 0, 0));
    deposit_base(&mut svm, &vault, custody, ONE_BASE);

    // +20% vs. the stored price: above the 10% bound.
    assert_roshi_error(
        send(
            &mut svm,
            report_nav_ix(&vault, 200_000, [1; 32]),
            &vault.roles.nav_authority,
        ),
        RoshiError::NavGainExceedsBound,
    );

    // The capped amount lands; the remainder rolls into a later report.
    report(&mut svm, &vault, 100_000, 2);
    assert_eq!(vault.load(&svm).total_assets, 1_100_000);

    // No downward bound: a -9% report lands in one go.
    report(&mut svm, &vault, 0, 3);
    assert_eq!(vault.load(&svm).total_assets, 1_000_000);
}

#[test]
fn test_report_gain_bound_skips_empty_vaults() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, _) = setup_vault(&mut svm, VaultControls::new(0, 0, 0, 0, 1_000, 0, 0));

    // No shares outstanding: any reported value passes (post-total-loss
    // recovery must not wedge).
    report(&mut svm, &vault, 5_000_000, 1);
    assert_eq!(vault.load(&svm).total_assets, 5_000_000);
}

#[test]
fn test_write_down_fees_unwedges_report_nav() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, custody) = setup_vault(&mut svm, VaultControls::default());
    deposit_base(&mut svm, &vault, custody, ONE_BASE);
    fund(&mut svm, &vault.roles.admin);

    // Losses ate into the fee cushion: gross (1_000_000 idle) no longer
    // covers the fee liability, so reporting wedges.
    let mut state = vault.load(&svm);
    state.fees_payable = 1_500_000;
    svm.set_account(
        vault.address,
        solana_sdk::account::Account {
            lamports: svm.minimum_balance_for_rent_exemption(roshi::state::vault::Vault::SPACE),
            data: wincode::serialize(&roshi::state::Account::Vault(state)).unwrap(),
            owner: roshi::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    assert_roshi_error(
        send(
            &mut svm,
            report_nav_ix(&vault, 0, [1; 32]),
            &vault.roles.nav_authority,
        ),
        RoshiError::InvalidVaultState,
    );

    // Forgiving part of the fee liability unwedges the report path; struck
    // tickets would have remained untouched throughout.
    send_ok(
        &mut svm,
        roshi_client::instruction::write_down_fees(
            vault.roles.admin.pubkey(),
            vault.address,
            600_000,
        )
        .unwrap(),
        &vault.roles.admin,
    );
    report(&mut svm, &vault, 0, 2);

    let state = vault.load(&svm);
    assert_eq!(state.fees_payable, 900_000);
    assert_eq!(state.total_assets, 100_000);
}

#[test]
fn test_report_rejects_reports_arriving_before_the_min_interval() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    set_clock_timestamp(&mut svm, 10_000);
    let (vault, _) = setup_vault(&mut svm, VaultControls::new(0, 0, 60, 0, 0, 0, 0));

    // The first report is exempt.
    report(&mut svm, &vault, 0, 1);

    set_clock_timestamp(&mut svm, 10_059);
    assert_roshi_error(
        send(
            &mut svm,
            report_nav_ix(&vault, 0, [2; 32]),
            &vault.roles.nav_authority,
        ),
        RoshiError::ReportTooFrequent,
    );

    set_clock_timestamp(&mut svm, 10_060);
    report(&mut svm, &vault, 0, 3);
    assert_eq!(vault.load(&svm).report_epoch, 2);
}
