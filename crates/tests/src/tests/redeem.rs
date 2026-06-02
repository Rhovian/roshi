//! `redeem`: burn shares, lock in the current share price, and queue a
//! withdrawal ticket for later payout by `process_withdrawals`. Redemptions are
//! asynchronous because vault assets are deployed off-chain, so the owed base
//! assets are carved out of `total_assets` into `pending_withdrawal_assets` and
//! the ticket records what is owed. litesvm runs the real SPL Token program, so
//! the share burn CPI executes end to end.

use litesvm::LiteSVM;
use roshi::{
    error::RoshiError,
    state::{
        sub_account::VaultSubAccount, withdrawal_ticket::WithdrawalTicket, Account as RoshiAccount,
    },
};
use solana_instruction::{error::InstructionError, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
use wincode::{deserialize, serialize};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, mint_supply, send, send_ok, set_ata,
    set_mint, set_token_account, setup_program, token_balance, TestVault, VaultBuilder,
};

/// One whole base unit at 6 decimals.
const ONE_BASE: u64 = 1_000_000;
/// Shares minted for an initial `ONE_BASE` deposit (`ONE_BASE * 10^9 / 10^6`).
const ONE_BASE_SHARES: u64 = 1_000_000_000;

/// A vault seeded with a single `ONE_BASE` base deposit, so the owner holds
/// `ONE_BASE_SHARES` and the vault accounting is `total_assets = ONE_BASE`,
/// `total_shares = ONE_BASE_SHARES` at a 1:1 redemption price.
struct RedeemFixture {
    vault: TestVault,
    share_mint: Pubkey,
    owner: Keypair,
    share_account: Pubkey,
    recipient: Pubkey,
    withdraw_sub_account: Pubkey,
}

fn setup_redeem(svm: &mut LiteSVM) -> RedeemFixture {
    let base_mint = Pubkey::new_unique();
    let share_mint = Pubkey::new_unique();
    let vault = VaultBuilder::new()
        .base_mint(base_mint)
        .share_mint(share_mint)
        .install(svm);
    set_mint(svm, share_mint, &vault.address, 9);
    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;
    let custody = set_ata(svm, &sub_account, &base_mint, 0);

    let owner = Keypair::new();
    fund(svm, &owner);
    let source = set_ata(svm, &owner.pubkey(), &base_mint, ONE_BASE);
    let share_account = set_ata(svm, &owner.pubkey(), &share_mint, 0);
    let recipient = Pubkey::new_unique();
    set_token_account(svm, recipient, &base_mint, &owner.pubkey(), 0);
    let withdraw_sub_account = VaultSubAccount::find_address(&vault.address, 1).0;

    send_ok(
        svm,
        roshi_client::instruction::deposit(
            owner.pubkey(),
            vault.address,
            source,
            custody,
            share_account,
            share_mint,
            base_mint,
            ONE_BASE,
            0,
            vec![],
            vec![],
        )
        .unwrap(),
        &owner,
    );
    svm.expire_blockhash();

    RedeemFixture {
        vault,
        share_mint,
        owner,
        share_account,
        recipient,
        withdraw_sub_account,
    }
}

/// Build a redeem instruction against the ticket PDA for `(vault,
/// recipient_token_account, ticket_index)`, returning the ticket address
/// alongside it.
fn redeem_ix(
    fixture: &RedeemFixture,
    ticket_index: u8,
    shares: u64,
    min_assets_out: u64,
) -> (Pubkey, Instruction) {
    let ticket =
        WithdrawalTicket::find_address(&fixture.vault.address, &fixture.recipient, ticket_index).0;
    let ix = roshi_client::instruction::redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.share_mint,
        fixture.recipient,
        ticket,
        ticket_index,
        shares,
        min_assets_out,
    )
    .unwrap();
    (ticket, ix)
}

fn load_ticket(svm: &LiteSVM, ticket: Pubkey) -> WithdrawalTicket {
    let account = svm.get_account(&ticket).unwrap();
    let RoshiAccount::WithdrawalTicket(ticket) = deserialize(&account.data).unwrap() else {
        panic!("expected withdrawal ticket account");
    };
    ticket
}

fn write_vault_state(
    svm: &mut LiteSVM,
    fixture: &RedeemFixture,
    state: roshi::state::vault::Vault,
) {
    svm.set_account(
        fixture.vault.address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(roshi::state::vault::Vault::SPACE),
            data: serialize(&RoshiAccount::Vault(state)).unwrap(),
            owner: roshi::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn cancel_redeem_ix(fixture: &RedeemFixture, ticket: Pubkey) -> Instruction {
    cancel_redeem_ix_with_min_shares(fixture, ticket, 0)
}

fn cancel_redeem_ix_with_min_shares(
    fixture: &RedeemFixture,
    ticket: Pubkey,
    min_shares_out: u64,
) -> Instruction {
    roshi_client::instruction::cancel_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        ticket,
        fixture.share_mint,
        fixture.share_account,
        min_shares_out,
    )
    .unwrap()
}

fn process_withdrawals_ix(
    fixture: &RedeemFixture,
    custody: Pubkey,
    settlements: Vec<(Pubkey, Pubkey, Pubkey)>,
) -> Instruction {
    roshi_client::instruction::process_withdrawals(
        fixture.vault.roles.withdrawal_authority.pubkey(),
        fixture.vault.address,
        fixture.withdraw_sub_account,
        custody,
        settlements,
    )
    .unwrap()
}

fn setup_withdraw_custody(svm: &mut LiteSVM, fixture: &RedeemFixture, amount: u64) -> Pubkey {
    set_ata(
        svm,
        &fixture.withdraw_sub_account,
        &fixture.vault.base_mint,
        amount,
    )
}

fn write_ticket(svm: &mut LiteSVM, ticket_key: Pubkey, ticket: WithdrawalTicket) {
    svm.set_account(
        ticket_key,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(WithdrawalTicket::SPACE),
            data: serialize(&RoshiAccount::WithdrawalTicket(ticket)).unwrap(),
            owner: roshi::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

#[test]
fn test_redeem_burns_shares_and_queues_ticket() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let shares = ONE_BASE_SHARES / 2;
    let (ticket, ix) = redeem_ix(&fixture, 0, shares, 0);
    send_ok(&mut svm, ix, &fixture.owner);

    // Half the shares are burned from the owner and removed from supply.
    assert_eq!(
        token_balance(&svm, &fixture.share_account),
        ONE_BASE_SHARES - shares
    );
    assert_eq!(
        mint_supply(&svm, &fixture.share_mint),
        ONE_BASE_SHARES - shares
    );

    // assets_owed = floor(shares * total_assets / total_shares) at 1:1 price.
    let assets_owed = ONE_BASE / 2;
    let queued = load_ticket(&svm, ticket);
    assert_eq!(queued.vault, fixture.vault.address.to_bytes());
    assert_eq!(queued.owner, fixture.owner.pubkey().to_bytes());
    assert_eq!(queued.recipient_token_account, fixture.recipient.to_bytes());
    assert_eq!(queued.ticket_index, 0);
    assert_eq!(queued.shares_burned, shares);
    assert_eq!(queued.assets_owed, assets_owed);

    // The owed assets are carved out of NAV into the pending bucket, leaving the
    // price intact for remaining holders.
    let state = fixture.vault.load(&svm);
    assert_eq!(state.total_shares, ONE_BASE_SHARES - shares);
    assert_eq!(state.total_assets, ONE_BASE - assets_owed);
    assert_eq!(state.pending_withdrawal_assets, assets_owed);
}

#[test]
fn test_process_withdrawals_pays_recipient_and_closes_ticket() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let shares = ONE_BASE_SHARES / 2;
    let assets_owed = ONE_BASE / 2;
    let (ticket, redeem) = redeem_ix(&fixture, 0, shares, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let custody = setup_withdraw_custody(&mut svm, &fixture, assets_owed);
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![(ticket, fixture.owner.pubkey(), fixture.recipient)],
    );
    send_ok(&mut svm, ix, &fixture.vault.roles.withdrawal_authority);

    assert_eq!(token_balance(&svm, &fixture.recipient), assets_owed);
    assert_eq!(token_balance(&svm, &custody), 0);
    assert!(svm.get_account(&ticket).is_none());

    let state = fixture.vault.load(&svm);
    assert_eq!(state.pending_withdrawal_assets, 0);
}

#[test]
fn test_process_withdrawals_can_partially_settle_batch() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let first_shares = ONE_BASE_SHARES / 4;
    let second_shares = ONE_BASE_SHARES / 4;
    let (first_ticket, first_redeem) = redeem_ix(&fixture, 0, first_shares, 0);
    send_ok(&mut svm, first_redeem, &fixture.owner);
    svm.expire_blockhash();
    let second_recipient = Pubkey::new_unique();
    set_token_account(
        &mut svm,
        second_recipient,
        &fixture.vault.base_mint,
        &fixture.owner.pubkey(),
        0,
    );
    let second_ticket =
        WithdrawalTicket::find_address(&fixture.vault.address, &second_recipient, 1).0;
    let second_redeem = roshi_client::instruction::redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.share_mint,
        second_recipient,
        second_ticket,
        1,
        second_shares,
        0,
    )
    .unwrap();
    send_ok(&mut svm, second_redeem, &fixture.owner);
    svm.expire_blockhash();

    let first_assets = ONE_BASE / 4;
    let custody = setup_withdraw_custody(&mut svm, &fixture, first_assets);
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![(first_ticket, fixture.owner.pubkey(), fixture.recipient)],
    );
    send_ok(&mut svm, ix, &fixture.vault.roles.withdrawal_authority);

    assert_eq!(token_balance(&svm, &fixture.recipient), first_assets);
    assert!(svm.get_account(&first_ticket).is_none());
    assert!(svm.get_account(&second_ticket).is_some());

    let state = fixture.vault.load(&svm);
    assert_eq!(state.pending_withdrawal_assets, ONE_BASE / 2 - first_assets);
}

#[test]
fn test_process_withdrawals_rejects_wrong_authority() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let custody = setup_withdraw_custody(&mut svm, &fixture, ONE_BASE / 2);
    let ix = roshi_client::instruction::process_withdrawals(
        outsider.pubkey(),
        fixture.vault.address,
        fixture.withdraw_sub_account,
        custody,
        vec![(ticket, fixture.owner.pubkey(), fixture.recipient)],
    )
    .unwrap();

    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );
}

#[test]
fn test_process_withdrawals_rejects_mismatched_ticket_data() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let (ticket_key, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let mut ticket = load_ticket(&svm, ticket_key);
    ticket.recipient_token_account = Pubkey::new_unique().to_bytes();
    write_ticket(&mut svm, ticket_key, ticket);

    let custody = setup_withdraw_custody(&mut svm, &fixture, ONE_BASE / 2);
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![(ticket_key, fixture.owner.pubkey(), fixture.recipient)],
    );

    assert_roshi_error(
        send(&mut svm, ix, &fixture.vault.roles.withdrawal_authority),
        RoshiError::InvalidWithdrawalTicketAccount,
    );
}

#[test]
fn test_process_withdrawals_rejects_duplicate_ticket_account() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let custody = setup_withdraw_custody(&mut svm, &fixture, ONE_BASE);
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![
            (ticket, fixture.owner.pubkey(), fixture.recipient),
            (ticket, fixture.owner.pubkey(), fixture.recipient),
        ],
    );

    assert_roshi_error(
        send(&mut svm, ix, &fixture.vault.roles.withdrawal_authority),
        RoshiError::InvalidWithdrawalTicketAccount,
    );
    assert_eq!(token_balance(&svm, &fixture.recipient), 0);
    assert!(svm.get_account(&ticket).is_some());
}

#[test]
fn test_process_withdrawals_rejects_insufficient_custody_balance_atomically() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let assets_owed = ONE_BASE / 2;
    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let custody = setup_withdraw_custody(&mut svm, &fixture, assets_owed - 1);
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![(ticket, fixture.owner.pubkey(), fixture.recipient)],
    );

    assert!(send(&mut svm, ix, &fixture.vault.roles.withdrawal_authority).is_err());
    assert_eq!(token_balance(&svm, &fixture.recipient), 0);
    assert_eq!(token_balance(&svm, &custody), assets_owed - 1);
    assert!(svm.get_account(&ticket).is_some());
    assert_eq!(
        fixture.vault.load(&svm).pending_withdrawal_assets,
        assets_owed
    );
}

#[test]
fn test_process_withdrawals_rejects_wrong_withdraw_subaccount() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let custody = setup_withdraw_custody(&mut svm, &fixture, ONE_BASE / 2);
    let wrong_withdraw_sub_account = VaultSubAccount::find_address(&fixture.vault.address, 2).0;
    let ix = roshi_client::instruction::process_withdrawals(
        fixture.vault.roles.withdrawal_authority.pubkey(),
        fixture.vault.address,
        wrong_withdraw_sub_account,
        custody,
        vec![(ticket, fixture.owner.pubkey(), fixture.recipient)],
    )
    .unwrap();

    assert_instruction_error(
        send(&mut svm, ix, &fixture.vault.roles.withdrawal_authority),
        InstructionError::InvalidSeeds,
    );
}

#[test]
fn test_process_withdrawals_rejects_custody_not_owned_by_withdraw_subaccount() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let custody = set_ata(
        &mut svm,
        &fixture.owner.pubkey(),
        &fixture.vault.base_mint,
        ONE_BASE / 2,
    );
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![(ticket, fixture.owner.pubkey(), fixture.recipient)],
    );

    assert_roshi_error(
        send(&mut svm, ix, &fixture.vault.roles.withdrawal_authority),
        RoshiError::InvalidTokenAccount,
    );
}

#[test]
fn test_process_withdrawals_rejects_destination_for_wrong_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    set_token_account(
        &mut svm,
        fixture.recipient,
        &fixture.share_mint,
        &fixture.owner.pubkey(),
        0,
    );
    let custody = setup_withdraw_custody(&mut svm, &fixture, ONE_BASE / 2);
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![(ticket, fixture.owner.pubkey(), fixture.recipient)],
    );

    assert_roshi_error(
        send(&mut svm, ix, &fixture.vault.roles.withdrawal_authority),
        RoshiError::InvalidTokenAccount,
    );
}

#[test]
fn test_process_withdrawals_allowed_while_withdrawals_paused() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);
    fund(&mut svm, &fixture.vault.roles.admin);
    fund(&mut svm, &fixture.vault.roles.withdrawal_authority);

    let assets_owed = ONE_BASE / 2;
    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    send_ok(
        &mut svm,
        roshi_client::instruction::set_pause_flags(
            fixture.vault.roles.admin.pubkey(),
            fixture.vault.address,
            false,
            true,
            false,
        )
        .unwrap(),
        &fixture.vault.roles.admin,
    );
    svm.expire_blockhash();

    let custody = setup_withdraw_custody(&mut svm, &fixture, assets_owed);
    let ix = process_withdrawals_ix(
        &fixture,
        custody,
        vec![(ticket, fixture.owner.pubkey(), fixture.recipient)],
    );
    send_ok(&mut svm, ix, &fixture.vault.roles.withdrawal_authority);

    assert_eq!(token_balance(&svm, &fixture.recipient), assets_owed);
    assert!(svm.get_account(&ticket).is_none());
    assert_eq!(fixture.vault.load(&svm).withdrawals_paused(), Ok(true));
}

#[test]
fn test_cancel_redeem_reenters_at_current_nav_and_closes_ticket() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let shares = ONE_BASE_SHARES / 2;
    let assets_owed = ONE_BASE / 2;
    let (ticket, redeem) = redeem_ix(&fixture, 0, shares, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    // Simulate gains in the active vault while the withdrawal is pending.
    let mut state = fixture.vault.load(&svm);
    state.total_assets = ONE_BASE;
    write_vault_state(&mut svm, &fixture, state);

    send_ok(&mut svm, cancel_redeem_ix(&fixture, ticket), &fixture.owner);

    // The pending claim re-enters at current active NAV:
    // floor(500k assets * 500M active shares / 1M active assets) = 250M.
    let reminted = shares / 2;
    assert_eq!(
        token_balance(&svm, &fixture.share_account),
        ONE_BASE_SHARES - shares + reminted
    );
    assert_eq!(
        mint_supply(&svm, &fixture.share_mint),
        ONE_BASE_SHARES - shares + reminted
    );

    let state = fixture.vault.load(&svm);
    assert_eq!(state.pending_withdrawal_assets, 0);
    assert_eq!(state.total_assets, ONE_BASE + assets_owed);
    assert_eq!(state.total_shares, ONE_BASE_SHARES - shares + reminted);
    assert!(svm.get_account(&ticket).is_none());
}

#[test]
fn test_cancel_redeem_restores_burned_shares_when_no_active_holders_remain() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    send_ok(&mut svm, cancel_redeem_ix(&fixture, ticket), &fixture.owner);

    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
    assert_eq!(mint_supply(&svm, &fixture.share_mint), ONE_BASE_SHARES);

    let state = fixture.vault.load(&svm);
    assert_eq!(state.pending_withdrawal_assets, 0);
    assert_eq!(state.total_assets, ONE_BASE);
    assert_eq!(state.total_shares, ONE_BASE_SHARES);
    assert!(svm.get_account(&ticket).is_none());
}

#[test]
fn test_cancel_redeem_rejects_non_owner() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let ix = roshi_client::instruction::cancel_redeem(
        outsider.pubkey(),
        fixture.vault.address,
        ticket,
        fixture.share_mint,
        fixture.share_account,
        0,
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &outsider),
        RoshiError::InvalidWithdrawalTicketAccount,
    );
    assert!(svm.get_account(&ticket).is_some());
}

#[test]
fn test_cancel_redeem_rejects_below_min_shares_out() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    assert_roshi_error(
        send(
            &mut svm,
            cancel_redeem_ix_with_min_shares(&fixture, ticket, ONE_BASE_SHARES),
            &fixture.owner,
        ),
        RoshiError::SlippageExceeded,
    );
    assert!(svm.get_account(&ticket).is_some());
}

#[test]
fn test_cancel_redeem_rejects_share_destination_for_wrong_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let wrong_mint = Pubkey::new_unique();
    set_mint(&mut svm, wrong_mint, &fixture.vault.address, 9);
    let wrong_share_dest = set_ata(&mut svm, &fixture.owner.pubkey(), &wrong_mint, 0);
    let ix = roshi_client::instruction::cancel_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        ticket,
        fixture.share_mint,
        wrong_share_dest,
        0,
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::InvalidTokenAccount,
    );
    assert!(svm.get_account(&ticket).is_some());
}

#[test]
fn test_cancel_redeem_rejects_share_destination_for_wrong_owner() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let other_owner = Pubkey::new_unique();
    let wrong_share_dest = set_ata(&mut svm, &other_owner, &fixture.share_mint, 0);
    let ix = roshi_client::instruction::cancel_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        ticket,
        fixture.share_mint,
        wrong_share_dest,
        0,
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::InvalidTokenAccount,
    );
    assert!(svm.get_account(&ticket).is_some());
}

#[test]
fn test_cancel_redeem_rejects_wrong_ticket_pda() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let wrong_ticket = Pubkey::new_unique();
    let ix = roshi_client::instruction::cancel_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        wrong_ticket,
        fixture.share_mint,
        fixture.share_account,
        0,
    )
    .unwrap();

    assert_instruction_error(
        send(&mut svm, ix, &fixture.owner),
        InstructionError::IllegalOwner,
    );
    assert!(svm.get_account(&ticket).is_some());
}

#[test]
fn test_cancel_redeem_rejects_ticket_data_with_mismatched_owner() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket_key, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let mut ticket = load_ticket(&svm, ticket_key);
    ticket.owner = Pubkey::new_unique().to_bytes();
    write_ticket(&mut svm, ticket_key, ticket);

    assert_roshi_error(
        send(
            &mut svm,
            cancel_redeem_ix(&fixture, ticket_key),
            &fixture.owner,
        ),
        RoshiError::InvalidWithdrawalTicketAccount,
    );
    assert!(svm.get_account(&ticket_key).is_some());
}

#[test]
fn test_cancel_redeem_rejects_ticket_data_with_mismatched_bump() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket_key, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    let mut ticket = load_ticket(&svm, ticket_key);
    ticket.bump = ticket.bump.wrapping_add(1);
    write_ticket(&mut svm, ticket_key, ticket);

    assert_instruction_error(
        send(
            &mut svm,
            cancel_redeem_ix(&fixture, ticket_key),
            &fixture.owner,
        ),
        InstructionError::InvalidSeeds,
    );
    assert!(svm.get_account(&ticket_key).is_some());
}

#[test]
fn test_cancel_redeem_allowed_while_withdrawals_paused() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, redeem) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, 0);
    send_ok(&mut svm, redeem, &fixture.owner);
    svm.expire_blockhash();

    fund(&mut svm, &fixture.vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::set_pause_flags(
            fixture.vault.roles.admin.pubkey(),
            fixture.vault.address,
            false,
            true,
            false,
        )
        .unwrap(),
        &fixture.vault.roles.admin,
    );
    svm.expire_blockhash();

    send_ok(&mut svm, cancel_redeem_ix(&fixture, ticket), &fixture.owner);
    assert!(svm.get_account(&ticket).is_none());
}

#[test]
fn test_redeem_all_shares_drains_vault_accounting() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (ticket, ix) = redeem_ix(&fixture, 0, ONE_BASE_SHARES, 0);
    send_ok(&mut svm, ix, &fixture.owner);

    assert_eq!(token_balance(&svm, &fixture.share_account), 0);
    assert_eq!(mint_supply(&svm, &fixture.share_mint), 0);

    let queued = load_ticket(&svm, ticket);
    assert_eq!(queued.shares_burned, ONE_BASE_SHARES);
    assert_eq!(queued.assets_owed, ONE_BASE);

    let state = fixture.vault.load(&svm);
    assert_eq!(state.total_shares, 0);
    assert_eq!(state.total_assets, 0);
    assert_eq!(state.pending_withdrawal_assets, ONE_BASE);
}

#[test]
fn test_redeem_rejects_when_withdrawals_paused() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    fund(&mut svm, &fixture.vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::set_pause_flags(
            fixture.vault.roles.admin.pubkey(),
            fixture.vault.address,
            false,
            true,
            false,
        )
        .unwrap(),
        &fixture.vault.roles.admin,
    );
    svm.expire_blockhash();

    let (_, ix) = redeem_ix(&fixture, 0, ONE_BASE_SHARES, 0);
    assert_roshi_error(send(&mut svm, ix, &fixture.owner), RoshiError::VaultPaused);
    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
}

#[test]
fn test_redeem_rejects_below_min_assets_out() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    // Half the shares are worth ONE_BASE / 2 assets; demand one more.
    let (_, ix) = redeem_ix(&fixture, 0, ONE_BASE_SHARES / 2, ONE_BASE / 2 + 1);
    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::SlippageExceeded,
    );
    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
}

#[test]
fn test_redeem_rejects_zero_shares() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (_, ix) = redeem_ix(&fixture, 0, 0, 0);
    assert_roshi_error(send(&mut svm, ix, &fixture.owner), RoshiError::ZeroOutput);
    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
}

#[test]
fn test_redeem_rejects_more_than_total_shares() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let (_, ix) = redeem_ix(&fixture, 0, ONE_BASE_SHARES + 1, 0);
    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::InvalidVaultState,
    );
    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
}

#[test]
fn test_redeem_rejects_wrong_ticket_pda() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    // A ticket account that is not the PDA for
    // (vault, recipient_token_account, ticket_index).
    let wrong_ticket = Pubkey::new_unique();
    let ix = roshi_client::instruction::redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.share_mint,
        fixture.recipient,
        wrong_ticket,
        0,
        ONE_BASE_SHARES,
        0,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &fixture.owner),
        InstructionError::InvalidSeeds,
    );
}

#[test]
fn test_redeem_rejects_wrong_share_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    // A valid mint that is not the vault's share mint.
    let other_mint = Pubkey::new_unique();
    set_mint(&mut svm, other_mint, &fixture.vault.address, 9);
    let ticket = WithdrawalTicket::find_address(&fixture.vault.address, &fixture.recipient, 0).0;
    let ix = roshi_client::instruction::redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        other_mint,
        fixture.recipient,
        ticket,
        0,
        ONE_BASE_SHARES,
        0,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::InvalidVaultState,
    );
}

#[test]
fn test_redeem_rejects_recipient_for_wrong_mint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let wrong_mint = Pubkey::new_unique();
    set_mint(&mut svm, wrong_mint, &fixture.vault.address, 9);
    let wrong_recipient = set_ata(&mut svm, &fixture.owner.pubkey(), &wrong_mint, 0);
    let ticket = WithdrawalTicket::find_address(&fixture.vault.address, &wrong_recipient, 0).0;
    let ix = roshi_client::instruction::redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.share_mint,
        wrong_recipient,
        ticket,
        0,
        ONE_BASE_SHARES,
        0,
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::InvalidTokenAccount,
    );
    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
}

#[test]
fn test_redeem_rejects_duplicate_ticket_index() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let fixture = setup_redeem(&mut svm);

    let quarter = ONE_BASE_SHARES / 4;
    let (_, first) = redeem_ix(&fixture, 0, quarter, 0);
    send_ok(&mut svm, first, &fixture.owner);
    svm.expire_blockhash();

    // Reusing ticket index 0 collides with the still-open ticket PDA.
    let (_, second) = redeem_ix(&fixture, 0, quarter, 0);
    assert_instruction_error(
        send(&mut svm, second, &fixture.owner),
        InstructionError::AccountAlreadyInitialized,
    );
}
