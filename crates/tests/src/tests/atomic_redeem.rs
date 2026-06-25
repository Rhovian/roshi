//! `atomic_redeem`: public user redemption through a pre-authorized unwind CPI.
//! The test venue CPI is an SPL Token transfer from a subaccount-owned venue
//! token account into vault custody; the wrapper bounds that transfer amount by
//! the user's share entitlement before invoking it.

use litesvm::LiteSVM;
use roshi::{
    error::RoshiError,
    instructions::{AccountFlags, AtomicRedeemArgs},
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Op, Ops},
        sub_account::VaultSubAccount,
        Account as RoshiAccount,
    },
    ID,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
use wincode::serialize;

use crate::helpers::{
    assert_roshi_error, associated_token_address_with_program, fund, mint_supply, send, send_ok,
    set_ata, set_ata_with_program, set_mint, set_token_2022_mint, set_token_account,
    set_token_account_with_program, setup_program, token_balance, VaultBuilder,
    TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};

const ONE_BASE: u64 = 1_000_000;
const ONE_BASE_SHARES: u64 = 1_000_000_000;
const REDEEM_SHARES: u64 = ONE_BASE_SHARES / 2;
const REDEEM_AMOUNT: u64 = ONE_BASE / 2;
const TRANSFER_AMOUNT_OFFSET: u16 = 1;

struct AtomicRedeemFixture {
    vault: crate::helpers::TestVault,
    owner: Keypair,
    share_account: Pubkey,
    recipient: Pubkey,
    sub_account_index: u8,
    sub_account: Pubkey,
    custody: Pubkey,
    venue_account: Pubkey,
    base_token_program: Pubkey,
    action_pda: Pubkey,
    action_hash: [u8; 32],
    ix_data: Vec<u8>,
    ops: Ops,
}

impl AtomicRedeemFixture {
    fn setup(svm: &mut LiteSVM) -> Self {
        Self::setup_with_base_program(svm, TOKEN_PROGRAM_ID)
    }

    fn setup_with_base_program(svm: &mut LiteSVM, base_token_program: Pubkey) -> Self {
        let builder = VaultBuilder::new();
        if base_token_program == TOKEN_2022_PROGRAM_ID {
            set_token_2022_mint(svm, builder.base_mint_key(), &builder.address().0, 6);
            set_mint(svm, builder.share_mint_key(), &builder.address().0, 9);
        } else {
            builder.install_mints(svm);
        }
        let vault = builder.install(svm);

        let owner = Keypair::new();
        fund(svm, &owner);
        let source = set_ata_with_program(
            svm,
            &owner.pubkey(),
            &vault.base_mint,
            ONE_BASE,
            base_token_program,
        );
        let share_account = set_ata(svm, &owner.pubkey(), &vault.share_mint, 0);
        let sub_account_index = 0;
        let sub_account = VaultSubAccount::find_address(&vault.address, sub_account_index).0;
        let custody = associated_token_address_with_program(
            &sub_account,
            &vault.base_mint,
            &base_token_program,
        );
        set_token_account_with_program(
            svm,
            custody,
            &vault.base_mint,
            &sub_account,
            0,
            base_token_program,
        );

        send_ok(
            svm,
            roshi_client::instruction::deposit(
                owner.pubkey(),
                vault.address,
                source,
                custody,
                share_account,
                vault.share_mint,
                base_token_program,
                vault.base_mint,
                ONE_BASE,
                0,
                vec![],
                vec![],
            )
            .unwrap(),
            &owner,
        );
        svm.expire_blockhash();

        let recipient = Pubkey::new_unique();
        set_token_account_with_program(
            svm,
            recipient,
            &vault.base_mint,
            &owner.pubkey(),
            0,
            base_token_program,
        );
        let venue_account = Pubkey::new_unique();
        set_token_account_with_program(
            svm,
            venue_account,
            &vault.base_mint,
            &sub_account,
            REDEEM_AMOUNT,
            base_token_program,
        );

        let ix_data = token_transfer_data(REDEEM_AMOUNT);
        // Canonical full-route authoring: pin both writable custodies the unwind
        // touches — the venue source (meta 0) and the base destination (meta 1) —
        // and the transfer discriminator (ix_data[0]). Only the per-redeem amount
        // (ix_data[1..9]) is left free, so one action is reusable across users
        // while a caller can neither substitute an account nor swap the
        // instruction (both would change the action hash).
        let ops = Ops::new([
            Op::IngestAccount { index: 0 },
            Op::IngestAccount { index: 1 },
            Op::IngestInstruction { offset: 0, len: 1 },
        ])
        .unwrap();
        let action_metas = token_transfer_metas(venue_account, custody, sub_account);
        let action_hash =
            compute_action_hash_from_metas(&base_token_program, &ops, &action_metas, &ix_data, &[])
                .unwrap();
        let action_pda = Action::find_address(&vault.address, &action_hash).0;

        Self {
            vault,
            owner,
            share_account,
            recipient,
            sub_account_index,
            sub_account,
            custody,
            venue_account,
            base_token_program,
            action_pda,
            action_hash,
            ix_data,
            ops,
        }
    }

    fn install_action(&self, svm: &mut LiteSVM, amount_offset: u16) {
        let (_, action_bump) = Action::find_address(&self.vault.address, &self.action_hash);
        svm.set_account(
            self.action_pda,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
                data: serialize(&RoshiAccount::Action(Action {
                    vault: self.vault.address.to_bytes(),
                    action_hash: self.action_hash,
                    ops: self.ops,
                    scope: ActionScope::AtomicRedeem,
                    fee_num: 0,
                    fee_den: 0,
                    redeem_amount_offset: amount_offset,
                    bump: action_bump,
                }))
                .unwrap(),
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }

    fn ix(&self, shares: u64, min_output: u64, ix_data: Vec<u8>) -> Instruction {
        roshi_client::instruction::atomic_redeem(
            self.owner.pubkey(),
            self.vault.address,
            self.share_account,
            self.vault.share_mint,
            self.recipient,
            self.custody,
            self.base_token_program,
            self.sub_account,
            self.action_pda,
            vec![
                AccountMeta::new(self.venue_account, false),
                AccountMeta::new(self.custody, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(self.base_token_program, false),
            ],
            AtomicRedeemArgs {
                shares,
                min_output,
                sub_account: self.sub_account_index,
                program_id: self.base_token_program.to_bytes(),
                accounts_start: 0,
                accounts_len: 3,
                account_flags: vec![
                    AccountFlags {
                        is_signer: false,
                        is_writable: true,
                    },
                    AccountFlags {
                        is_signer: false,
                        is_writable: true,
                    },
                    AccountFlags {
                        is_signer: false,
                        is_writable: false,
                    },
                ],
                ix_data,
            },
        )
        .unwrap()
    }
}

fn write_vault_state(
    svm: &mut LiteSVM,
    fixture: &AtomicRedeemFixture,
    state: roshi::state::vault::Vault,
) {
    svm.set_account(
        fixture.vault.address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(roshi::state::vault::Vault::SPACE),
            data: serialize(&RoshiAccount::Vault(state)).unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

#[test]
fn test_atomic_redeem_rejects_stale_nav_report() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    let mut state = fixture.vault.load(&svm);
    state.controls = roshi::state::vault::VaultControls::new(0, 100, 0, 0, 0, 0, 0);
    state.report_epoch = 1;
    state.last_update_ts = 1_000;
    write_vault_state(&mut svm, &fixture, state);

    // One second past max_report_age_secs: the atomic exit would escape an
    // incurred-but-unreported loss, so it rejects.
    crate::helpers::set_clock_timestamp(&mut svm, 1_101);
    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, 0, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::StaleNavReport,
    );

    // At exactly the configured age the report is still fresh.
    svm.expire_blockhash();
    crate::helpers::set_clock_timestamp(&mut svm, 1_100);
    send_ok(
        &mut svm,
        fixture.ix(REDEEM_SHARES, 0, fixture.ix_data.clone()),
        &fixture.owner,
    );
    assert_eq!(token_balance(&svm, &fixture.recipient), REDEEM_AMOUNT);
}

#[test]
fn test_atomic_redeem_entitlement_uses_effective_nav() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    // Half the NAV is still-locked profit, half-way through its drip:
    // effective NAV = 750_000, so half the shares entitle ~375_000 — below
    // the 500_000 this unwind CPI would pay out.
    let mut state = fixture.vault.load(&svm);
    state.locked_profit = 500_000;
    state.profit_unlock_start_ts = 1_000;
    state.profit_unlock_end_ts = 2_000;
    write_vault_state(&mut svm, &fixture, state);
    crate::helpers::set_clock_timestamp(&mut svm, 1_500);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, 0, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::WithdrawalExceedsEntitlement,
    );
}

#[test]
fn test_atomic_redeem_charges_exit_fee_to_the_pool() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    let mut state = fixture.vault.load(&svm);
    state.controls = roshi::state::vault::VaultControls::new(0, 0, 0, 0, 0, 100, 0);
    write_vault_state(&mut svm, &fixture, state);

    send_ok(
        &mut svm,
        fixture.ix(REDEEM_SHARES, 0, fixture.ix_data.clone()),
        &fixture.owner,
    );

    // 1% of the 500_000 realized proceeds stays in custody for the pool.
    let fee = REDEEM_AMOUNT / 100;
    let payout = REDEEM_AMOUNT - fee;
    assert_eq!(token_balance(&svm, &fixture.recipient), payout);
    assert_eq!(token_balance(&svm, &fixture.custody), ONE_BASE + fee);

    let state = fixture.vault.load(&svm);
    // NAV drops by the payout only: the retained fee accrues to the
    // remaining holders' share price (full shares were burned).
    assert_eq!(state.total_assets, ONE_BASE - payout);
    assert_eq!(
        mint_supply(&svm, &fixture.vault.share_mint),
        ONE_BASE_SHARES - REDEEM_SHARES
    );
}

#[test]
fn test_atomic_redeem_min_output_applies_to_net_payout() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    let mut state = fixture.vault.load(&svm);
    state.controls = roshi::state::vault::VaultControls::new(0, 0, 0, 0, 0, 100, 0);
    write_vault_state(&mut svm, &fixture, state);

    // Gross proceeds meet min_output but the net payout does not.
    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::SlippageExceeded,
    );
}

#[test]
fn test_atomic_redeem_rejects_unbound_custody_route() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    // The finding's precondition: a broad action that pins only the program id
    // (empty ops), leaving every CPI account caller-controlled. The unwind's
    // writable sub-account custodies (venue source, base destination) are then
    // unbound, so a public caller could redirect the route to drain a sibling.
    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.ops = Ops::empty();
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &token_transfer_metas(fixture.venue_account, fixture.custody, fixture.sub_account),
        &fixture.ix_data,
        &[],
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, 0, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::UnboundAtomicRedeemAccount,
    );
}

#[test]
fn test_atomic_redeem_rejects_unbound_destination_redirect() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    // A partially-bound action that ingests only the venue source (index 0),
    // leaving the CPI destination unpinned. Because the action hash folds only
    // ingested accounts, a public caller can repoint the destination at their own
    // token account — draining the venue while the measured base custody never
    // moves (received = 0, NAV debited 0). Binding *every* writable meta rejects
    // this, not just sub-account custody.
    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    let attacker_dest = Pubkey::new_unique();
    set_token_account_with_program(
        &mut svm,
        attacker_dest,
        &fixture.vault.base_mint,
        &Pubkey::new_unique(),
        0,
        fixture.base_token_program,
    );

    fixture.ops = Ops::new([Op::IngestAccount { index: 0 }]).unwrap();
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &token_transfer_metas(fixture.venue_account, attacker_dest, fixture.sub_account),
        &fixture.ix_data,
        &[],
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    let ix = roshi_client::instruction::atomic_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.vault.share_mint,
        fixture.recipient,
        fixture.custody,
        fixture.base_token_program,
        fixture.sub_account,
        fixture.action_pda,
        vec![
            AccountMeta::new(fixture.venue_account, false),
            AccountMeta::new(attacker_dest, false),
            AccountMeta::new_readonly(fixture.sub_account, false),
            AccountMeta::new_readonly(fixture.base_token_program, false),
        ],
        AtomicRedeemArgs {
            shares: REDEEM_SHARES,
            min_output: 0,
            sub_account: fixture.sub_account_index,
            program_id: fixture.base_token_program.to_bytes(),
            accounts_start: 0,
            accounts_len: 3,
            account_flags: vec![
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: false,
                },
            ],
            ix_data: fixture.ix_data.clone(),
        },
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::UnboundAtomicRedeemAccount,
    );
    // The venue keeps its full balance: the redirect never executed.
    assert_eq!(token_balance(&svm, &fixture.venue_account), REDEEM_AMOUNT);
    assert_eq!(token_balance(&svm, &attacker_dest), 0);
}

#[test]
fn test_atomic_redeem_rejects_instruction_swap_on_bound_route() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    // The canonical action pins accounts AND the transfer discriminator, leaving
    // only the amount free. Attempting to swap the pinned SPL transfer for a
    // `SetAuthority` on the same venue — to seize it and drain it later — changes
    // the committed discriminator (and account layout), so the recomputed action
    // hash no longer matches and the relay rejects it. This is what makes the
    // public scope safe once the route is fully authored.
    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    let ix = roshi_client::instruction::atomic_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.vault.share_mint,
        fixture.recipient,
        fixture.custody,
        fixture.base_token_program,
        fixture.sub_account,
        fixture.action_pda,
        vec![
            AccountMeta::new(fixture.venue_account, false),
            AccountMeta::new_readonly(fixture.sub_account, false),
            AccountMeta::new_readonly(fixture.base_token_program, false),
        ],
        AtomicRedeemArgs {
            shares: REDEEM_SHARES,
            min_output: 0,
            sub_account: fixture.sub_account_index,
            program_id: fixture.base_token_program.to_bytes(),
            accounts_start: 0,
            accounts_len: 2,
            account_flags: vec![
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: false,
                },
            ],
            // New authority with small leading bytes so the amount decoded at
            // `redeem_amount_offset` clears the entitlement check, letting the
            // request reach (and fail) the action-hash comparison rather than the
            // earlier entitlement guard.
            ix_data: set_account_owner_data(Pubkey::new_from_array([0u8; 32])),
        },
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::UnauthorizedAction,
    );
    assert_eq!(token_balance(&svm, &fixture.venue_account), REDEEM_AMOUNT);
}

#[test]
fn test_atomic_redeem_happy_path() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    send_ok(
        &mut svm,
        fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT, fixture.ix_data.clone()),
        &fixture.owner,
    );

    assert_eq!(token_balance(&svm, &fixture.recipient), REDEEM_AMOUNT);
    assert_eq!(
        token_balance(&svm, &fixture.share_account),
        ONE_BASE_SHARES - REDEEM_SHARES
    );
    assert_eq!(
        mint_supply(&svm, &fixture.vault.share_mint),
        ONE_BASE_SHARES - REDEEM_SHARES
    );
    assert_eq!(
        fixture.vault.load(&svm).total_assets,
        ONE_BASE - REDEEM_AMOUNT
    );
}

#[test]
fn test_atomic_redeem_happy_path_with_token_2022_base() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup_with_base_program(&mut svm, TOKEN_2022_PROGRAM_ID);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    send_ok(
        &mut svm,
        fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT, fixture.ix_data.clone()),
        &fixture.owner,
    );

    assert_eq!(token_balance(&svm, &fixture.recipient), REDEEM_AMOUNT);
    assert_eq!(token_balance(&svm, &fixture.custody), ONE_BASE);
    assert_eq!(
        fixture.vault.load(&svm).total_assets,
        ONE_BASE - REDEEM_AMOUNT
    );
}

#[test]
fn test_atomic_redeem_rejects_withdrawal_amount_above_entitlement() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES / 2, REDEEM_AMOUNT, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::WithdrawalExceedsEntitlement,
    );
}

#[test]
fn test_atomic_redeem_rejects_realized_output_above_entitlement() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    let transfer_amount = REDEEM_AMOUNT + 1;
    let declared_amount = REDEEM_AMOUNT;
    set_token_account(
        &mut svm,
        fixture.venue_account,
        &fixture.vault.base_mint,
        &fixture.sub_account,
        transfer_amount,
    );

    let mut ix_data = token_transfer_data(transfer_amount);
    ix_data.extend_from_slice(&declared_amount.to_le_bytes());
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &token_transfer_metas(fixture.venue_account, fixture.custody, fixture.sub_account),
        &ix_data,
        &[],
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, 9);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, 0, ix_data),
            &fixture.owner,
        ),
        RoshiError::WithdrawalExceedsEntitlement,
    );
}

#[test]
fn test_atomic_redeem_rejects_received_below_min_output() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT + 1, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::SlippageExceeded,
    );
}

#[test]
fn test_atomic_redeem_rejects_when_withdrawals_paused() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);
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

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::VaultPaused,
    );
}

#[test]
fn test_atomic_redeem_rejects_share_account_in_cpi_metas() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    let ix_data = token_transfer_data(REDEEM_AMOUNT);
    let malicious_metas = vec![
        AccountMeta::new(fixture.venue_account, false),
        AccountMeta::new(fixture.custody, false),
        AccountMeta::new_readonly(fixture.sub_account, true),
        AccountMeta::new_readonly(fixture.share_account, false),
    ];
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &malicious_metas,
        &ix_data,
        &[],
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    let ix = roshi_client::instruction::atomic_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.vault.share_mint,
        fixture.recipient,
        fixture.custody,
        crate::helpers::TOKEN_PROGRAM_ID,
        fixture.sub_account,
        fixture.action_pda,
        vec![
            AccountMeta::new(fixture.venue_account, false),
            AccountMeta::new(fixture.custody, false),
            AccountMeta::new_readonly(fixture.sub_account, false),
            AccountMeta::new_readonly(fixture.share_account, false),
            AccountMeta::new_readonly(crate::helpers::TOKEN_PROGRAM_ID, false),
        ],
        AtomicRedeemArgs {
            shares: REDEEM_SHARES,
            min_output: REDEEM_AMOUNT,
            sub_account: fixture.sub_account_index,
            program_id: crate::helpers::TOKEN_PROGRAM_ID.to_bytes(),
            accounts_start: 0,
            accounts_len: 4,
            account_flags: vec![
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: false,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: false,
                },
            ],
            ix_data,
        },
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::UnauthorizedAction,
    );
}

#[test]
fn test_atomic_redeem_rejects_post_cpi_custody_owner_hijack() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    let mut new_owner = [0u8; 32];
    new_owner[31] = 1;
    let new_owner = Pubkey::from(new_owner);
    let ix_data = set_account_owner_data(new_owner);
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &set_account_owner_metas(fixture.custody, fixture.sub_account),
        &ix_data,
        &[],
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, 3);

    let ix = roshi_client::instruction::atomic_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.vault.share_mint,
        fixture.recipient,
        fixture.custody,
        crate::helpers::TOKEN_PROGRAM_ID,
        fixture.sub_account,
        fixture.action_pda,
        vec![
            AccountMeta::new(fixture.custody, false),
            AccountMeta::new_readonly(fixture.sub_account, false),
            AccountMeta::new_readonly(crate::helpers::TOKEN_PROGRAM_ID, false),
        ],
        AtomicRedeemArgs {
            shares: REDEEM_SHARES,
            min_output: 0,
            sub_account: fixture.sub_account_index,
            program_id: crate::helpers::TOKEN_PROGRAM_ID.to_bytes(),
            accounts_start: 0,
            accounts_len: 2,
            account_flags: vec![
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: false,
                },
            ],
            ix_data,
        },
    )
    .unwrap();

    assert_roshi_error(
        send(&mut svm, ix, &fixture.owner),
        RoshiError::InvalidTokenAccount,
    );
    assert_eq!(
        token_account_owner(&svm, fixture.custody),
        fixture.sub_account
    );
    assert_eq!(token_balance(&svm, &fixture.recipient), 0);
    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
    assert_eq!(
        mint_supply(&svm, &fixture.vault.share_mint),
        ONE_BASE_SHARES
    );
}

fn token_transfer_data(amount: u64) -> Vec<u8> {
    let mut data = vec![3];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn token_transfer_metas(
    source: Pubkey,
    destination: Pubkey,
    authority: Pubkey,
) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(source, false),
        AccountMeta::new(destination, false),
        AccountMeta::new_readonly(authority, true),
    ]
}

fn set_account_owner_data(owner: Pubkey) -> Vec<u8> {
    let mut data = vec![6, 2, 1];
    data.extend_from_slice(owner.as_ref());
    data
}

fn set_account_owner_metas(account: Pubkey, authority: Pubkey) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(account, false),
        AccountMeta::new_readonly(authority, true),
    ]
}

fn token_account_owner(svm: &LiteSVM, address: Pubkey) -> Pubkey {
    let account = svm.get_account(&address).unwrap();
    Pubkey::try_from(&account.data[32..64]).unwrap()
}
