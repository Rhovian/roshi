//! Crucible invariant-fuzzing harness for the Roshi vault program.
//!
//! Drives the real program through `roshi-client`-built (wincode-encoded)
//! instructions via `TestContext::raw_call`, and checks accounting invariants
//! after every mutated action sequence. Coverage comes from sBPF edge tracing
//! on the LiteSVM execution, so no program instrumentation is needed.
//!
//! Headline invariant: **base-token conservation**. The program mints/burns
//! only *shares*; it never creates or destroys base tokens. So the sum of every
//! base-token balance in the system must equal the amount installed at setup.

use anchor_lang::prelude::Clock;
use crucible_fuzzer::*;
use std::rc::Rc;

use roshi::{
    instructions::{
        AccountFlags, AtomicRedeemArgs, InitializeAssetArgs, InitializeVaultArgs, ManageArgs,
        SwapArgs, UpdateAssetArgs, UpdateVaultConfigArgs,
    },
    oracle::{OracleConfig, PythOracleConfig},
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Op, Ops},
        asset::Asset,
        external_destination::ExternalDestination,
        program_config::ProgramConfig,
        sub_account::VaultSubAccount,
        vault::{Vault, VaultControls},
        withdrawal_ticket::{WithdrawalTicket, WITHDRAWAL_STRIKE_DELAY_EPOCHS},
        Account as RoshiAccount,
    },
    ID,
};
use roshi_interface::{
    access::{access_merkle_leaf, access_merkle_node, verify_access_merkle_proof},
    find_share_mint_address,
    math::{assets_for_redeem, performance_fee_for_nav, shares_for_deposit},
};
use solana_account::Account;
use solana_instruction::AccountMeta;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

/// SPL Token `Transfer` instruction discriminator (classic token program).
const SPL_TRANSFER_TAG: u8 = 3;

mod support;
use support::{
    mint_supply, pyth_price_data, set_ata, set_ata_with_program, set_mint, set_pyth_price,
    set_token_2022_mint, set_token_account, set_token_account_with_program,
    set_transfer_fee_token_2022_mint, token_balance,
};

const NUM_USERS: usize = 3;
const TICKETS_PER_USER: u8 = 3;
const BASE_DECIMALS: u8 = 6;
/// Base each user starts with (1000 units at 6 decimals).
const INITIAL_USER_BASE: u64 = 1_000_000_000;
/// Deployed venue capital the `atomic_redeem` unwind can pull from. Generous so
/// the payout path stays reachable for the whole run.
const VENUE_BASE: u64 = 3_000_000_000;
const PERF_FEE_BPS: u16 = 100;
const WITHDRAWAL_BUFFER_BPS: u16 = 250;
const MAX_BPS: u16 = 10_000;
const FUND_LAMPORTS: u64 = 100_000_000_000;
/// Wall-time advanced per slot by `action_advance_slots` (oracle staleness is
/// `unix_timestamp`-based, so slots alone don't age a price).
const SECONDS_PER_SLOT: i64 = 1;

// Registered non-base asset, priced through a mock Pyth feed.
const ASSET_DECIMALS: u8 = 6;
/// Asset each user starts with (1000 units at 6 decimals).
const INITIAL_USER_ASSET: u64 = 1_000_000_000;
const PYTH_FEED_ID: [u8; 32] = [7u8; 32];
/// 2.0 base per asset unit at exponent -8 / output decimals 8.
const PYTH_BASE_PRICE: i64 = 200_000_000;
const PYTH_EXPONENT: i32 = -8;
const PYTH_PRICE_DECIMALS: u8 = 8;
/// Generous enough that fresh-price deposits dominate, small enough that a few
/// `advance_slots` calls within one sequence age the price into staleness.
const PYTH_MAX_AGE_SECS: u64 = 64;
/// Confidence ceiling: 5% of price. The setup price has conf 0 (passes); the
/// wide-conf negative installs conf == price (10_000 bps, fails).
const PYTH_MAX_CONF_BPS: u16 = 500;

#[derive(Clone)]
struct FuzzUser {
    kp: Rc<Keypair>,
    base_ata: Pubkey,
    share_ata: Pubkey,
    /// The user's account for the registered non-base asset (outsider: empty).
    asset_ata: Pubkey,
    /// The user's account for the registered bare Token-2022 asset (outsider:
    /// empty).
    token_2022_asset_ata: Pubkey,
    /// Access proof for the members tree. Members carry a proof that verifies
    /// against `members_root`; the outsider carries a stolen member proof that
    /// must not verify for its own identity.
    access_proof: Vec<[u8; 32]>,
}

#[derive(Clone)]
struct RoshiFixture {
    ctx: TestContext,
    program_id: Pubkey,
    config_pda: Pubkey,
    /// Program authority + vault admin + fee payer.
    operator: Rc<Keypair>,
    program_authority_alt: Rc<Keypair>,
    vault_authority_alt: Rc<Keypair>,
    /// Distinct vault roles. The corresponding `*_alt` keypairs are used by
    /// role-management actions that rotate the role, prove the old signer stops
    /// working, then restore the original signer.
    strategist: Rc<Keypair>,
    strategist_alt: Rc<Keypair>,
    nav_authority: Rc<Keypair>,
    nav_authority_alt: Rc<Keypair>,
    withdrawal_authority: Rc<Keypair>,
    withdrawal_authority_alt: Rc<Keypair>,
    external_authority: Rc<Keypair>,
    vault: Pubkey,
    share_mint: Pubkey,
    base_mint: Pubkey,
    treasury: Pubkey,
    /// Deposit and withdraw sub-accounts are deliberately distinct, so
    /// `report_nav` must read both canonical base ATAs and withdrawals must pay
    /// from the withdraw side.
    sub_account: Pubkey,
    custody: Pubkey,
    withdraw_sub_account: Pubkey,
    withdraw_custody: Pubkey,
    external_account: Pubkey,
    external_destination: Pubkey,
    /// Pre-authorized Manager action: an SPL token transfer custody -> external
    /// signed by the sub-account PDA, with the amount left free. Drives the
    /// arbitrary-CPI authorization machinery (`manage`, `validate_authorized_cpi`,
    /// `invoke_authorized_cpi`, the custody clean-check) through real CPI.
    manage_action: Pubkey,
    /// Pre-authorized Manager action that rebalances idle base from deposit
    /// custody to withdraw custody, unlocking the split-custody withdrawal path.
    rebalance_to_withdraw_action: Pubkey,
    /// Second base custody owned by the sub-account, the `swap` output leg.
    swap_custody: Pubkey,
    /// Authorized Swap actions moving base between the two custodies, one per
    /// direction so the fuzzer can't permanently strand base out of the
    /// withdrawal-paying custody. Same-mint "swaps" are degenerate but exercise
    /// all of `try_swap` (input/output bounds, custody reverify, the CPI).
    swap_forward_action: Pubkey,
    swap_reverse_action: Pubkey,
    /// Sub-account-owned base account standing in for deployed venue capital,
    /// the source the `atomic_redeem` unwind CPI pulls into custody.
    atomic_venue: Pubkey,
    /// Authorized AtomicRedeem action (empty ops: bounded by the on-chain
    /// entitlement and the custody-increase check, not the action hash).
    atomic_action: Pubkey,
    /// A revocable Manager action (custody -> treasury) and its hash, toggled by
    /// `action_revoke_action` to drive `revoke_action` and prove a revoked
    /// action can no longer move funds.
    revocable_action: Pubkey,
    revocable_action_hash: [u8; 32],
    /// Access merkle root over all `users` (every user is whitelisted, so the
    /// core deposit loop survives the vault being private). `set_vault_access`
    /// toggles the vault between private+this-root and public+zero.
    members_root: [u8; 32],
    /// A wallet absent from `members_root`: its deposits must be rejected while
    /// the vault is private, no matter what proof it submits.
    outsider: FuzzUser,
    users: Vec<FuzzUser>,
    /// Every base-token account in the system, for the conservation sum.
    base_accounts: Vec<Pubkey>,
    /// Total base installed at setup; conserved for the run's lifetime.
    initial_base: u128,
    /// Registered non-base asset priced through the mock Pyth feed: its mint,
    /// Asset PDA, sub-account custody (ATA), and the `PriceUpdateV2` account.
    asset_mint: Pubkey,
    asset_pda: Pubkey,
    asset_custody: Pubkey,
    pyth_account: Pubkey,
    /// Every asset-mint token account, for the asset conservation sum. A
    /// separate conserved quantity from base: non-base deposits move asset
    /// tokens here and credit `total_assets` in *priced base terms*, so asset
    /// atoms must never be folded into the base sum.
    asset_accounts: Vec<Pubkey>,
    /// Total asset installed at setup; conserved for the run's lifetime.
    initial_asset: u128,
    /// Registered bare Token-2022 asset, plus an extended Token-2022 mint whose
    /// `initialize_asset` attempt must be rejected.
    token_2022_asset_mint: Pubkey,
    token_2022_asset_pda: Pubkey,
    token_2022_asset_custody: Pubkey,
    token_2022_swap_custody: Pubkey,
    token_2022_swap_forward_action: Pubkey,
    token_2022_swap_reverse_action: Pubkey,
    transfer_fee_token_2022_mint: Pubkey,
    transfer_fee_token_2022_asset_pda: Pubkey,
    token_2022_asset_accounts: Vec<Pubkey>,
    initial_token_2022_asset: u128,
    /// Monotonic source of unique report hashes (avoids replay rejection so NAV
    /// reports actually advance the epoch, which is what prices withdrawals).
    report_nonce: u64,
    /// Highest `high_watermark` seen across this lineage's invariant checks; the
    /// watermark must never regress (a reset double-charges performance fees).
    prev_high_watermark: u64,
}

/// Submit `ix` signed by `signers`; returns whether it succeeded.
fn submit(
    ctx: &mut TestContext,
    ix: solana_instruction::Instruction,
    signers: &[&Keypair],
) -> bool {
    ctx.raw_call(ix)
        .signers(signers)
        .send()
        .map(|o| o.is_success())
        .unwrap_or(false)
}

/// Submit an instruction that must succeed during setup; panic loudly otherwise.
fn submit_ok(
    ctx: &mut TestContext,
    ix: solana_instruction::Instruction,
    signers: &[&Keypair],
    what: &str,
) {
    let outcome = ctx.raw_call(ix).signers(signers).send();
    match outcome {
        Ok(o) if o.is_success() => {}
        Ok(o) => panic!("setup step `{what}` failed:\n{}", o.logs().join("\n")),
        Err(e) => panic!("setup step `{what}` errored: {e:?}"),
    }
}

include!("fixture.rs");

fn pad_tag(tag: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..tag.len()].copy_from_slice(tag);
    out
}

/// Build a directionless access Merkle root over `leaves` plus each leaf's
/// proof, using the program's own `access_merkle_node` (it sorts each pair, so
/// proofs carry no direction bits). An odd node at any level is promoted
/// unchanged to the next level. `proofs[i]` verifies the owner of `leaves[i]`
/// against the returned root — folding the leaf through its proof reproduces it.
fn build_access_tree(leaves: &[[u8; 32]]) -> ([u8; 32], Vec<Vec<[u8; 32]>>) {
    let mut proofs = vec![Vec::new(); leaves.len()];
    // Each entry is a subtree: its node hash and the original leaf indices under it.
    let mut level: Vec<([u8; 32], Vec<usize>)> = leaves
        .iter()
        .enumerate()
        .map(|(i, leaf)| (*leaf, vec![i]))
        .collect();

    while level.len() > 1 {
        let mut next: Vec<([u8; 32], Vec<usize>)> = Vec::with_capacity(level.len().div_ceil(2));
        let mut iter = level.into_iter();
        while let Some((a_hash, a_leaves)) = iter.next() {
            match iter.next() {
                Some((b_hash, b_leaves)) => {
                    // Each side gains the other's hash as its next proof sibling.
                    for &i in &a_leaves {
                        proofs[i].push(b_hash);
                    }
                    for &i in &b_leaves {
                        proofs[i].push(a_hash);
                    }
                    let combined = access_merkle_node(&a_hash, &b_hash);
                    let mut union = a_leaves;
                    union.extend(b_leaves);
                    next.push((combined, union));
                }
                None => next.push((a_hash, a_leaves)), // odd node: promote unchanged
            }
        }
        level = next;
    }

    (level[0].0, proofs)
}

/// Authorize a transfer-only action (`Manager` or `Swap` scope) that moves base
/// `input -> output`, where `input` is owned by `sub_account` (the transfer
/// source, with `sub_account` as the signing authority; `output` may be any base
/// token account — `swap` additionally requires it to be sub-account-owned).
/// Pins the three accounts and the transfer discriminator, leaving only the
/// amount free. Returns the Action PDA and its hash (needed to revoke it later).
fn authorize_transfer_action(
    ctx: &mut TestContext,
    operator: &Keypair,
    vault: Pubkey,
    sub_account: Pubkey,
    input: Pubkey,
    output: Pubkey,
    scope: ActionScope,
) -> (Pubkey, [u8; 32]) {
    authorize_transfer_action_with_program(
        ctx,
        operator,
        vault,
        sub_account,
        input,
        output,
        scope,
        support::TOKEN_PROGRAM_ID,
    )
}

fn authorize_transfer_action_with_program(
    ctx: &mut TestContext,
    operator: &Keypair,
    vault: Pubkey,
    sub_account: Pubkey,
    input: Pubkey,
    output: Pubkey,
    scope: ActionScope,
    token_program: Pubkey,
) -> (Pubkey, [u8; 32]) {
    let ops = Ops::new([
        Op::IngestAccount { index: 0 },
        Op::IngestAccount { index: 1 },
        Op::IngestAccount { index: 2 },
        Op::IngestInstruction { offset: 0, len: 1 },
    ])
    .expect("ops within capacity");
    let metas = vec![
        AccountMeta::new(input, false),
        AccountMeta::new(output, false),
        AccountMeta::new_readonly(sub_account, true),
    ];
    // Ops ingest the three accounts and ix_data[0..1] (the transfer
    // discriminator), so only the amount appended after it is free.
    let action_hash =
        compute_action_hash_from_metas(&token_program, &ops, &metas, &[SPL_TRANSFER_TAG], &[])
            .expect("action hash");
    let (action, _) = Action::find_address(&vault, &action_hash);
    submit_ok(
        ctx,
        roshi_client::instruction::authorize_action(
            operator.pubkey(),
            vault,
            action,
            action_hash,
            scope,
            ops,
            0,
            0,
            0,
        )
        .unwrap(),
        &[operator],
        "authorize_action(transfer)",
    );
    (action, action_hash)
}

#[invariant_test]
fn invariant_core(fixture: &mut RoshiFixture) {
    // 1. Base-token conservation: shares are minted/burned, base never is.
    let total_base: u128 = fixture
        .base_accounts
        .iter()
        .map(|a| token_balance(&fixture.ctx.svm, a) as u128)
        .sum();
    fuzz_assert_eq!(
        total_base,
        fixture.initial_base,
        "base tokens not conserved: {} present vs {} installed",
        total_base,
        fixture.initial_base
    );

    // 1b. Asset-token conservation: the registered non-base asset is its own
    //     conserved quantity. Non-base deposits move asset atoms (this sum) and
    //     credit `total_assets` in priced base terms (invisible here), so the
    //     two sums stay independent.
    let total_asset: u128 = fixture
        .asset_accounts
        .iter()
        .map(|a| token_balance(&fixture.ctx.svm, a) as u128)
        .sum();
    fuzz_assert_eq!(
        total_asset,
        fixture.initial_asset,
        "asset tokens not conserved: {} present vs {} installed",
        total_asset,
        fixture.initial_asset
    );

    // 1c. Token-2022 asset conservation. Same economic pricing path as the
    // classic registered asset, but a distinct mint/program/account set.
    let total_token_2022_asset: u128 = fixture
        .token_2022_asset_accounts
        .iter()
        .map(|a| token_balance(&fixture.ctx.svm, a) as u128)
        .sum();
    fuzz_assert_eq!(
        total_token_2022_asset,
        fixture.initial_token_2022_asset,
        "Token-2022 asset tokens not conserved: {} present vs {} installed",
        total_token_2022_asset,
        fixture.initial_token_2022_asset
    );

    // 2. Vault structural invariants on the deserialized state.
    let account = fixture
        .ctx
        .get_account(&fixture.vault)
        .expect("vault account must exist");
    let vault = Vault::from_account_data(&account.data).expect("vault must deserialize");

    fuzz_assert!(
        vault.performance_fee_bps <= MAX_BPS && vault.withdrawal_buffer_bps <= MAX_BPS,
        "fee bps out of range: perf={} buffer={}",
        vault.performance_fee_bps,
        vault.withdrawal_buffer_bps
    );

    // 2b. High-watermark monotonicity. The performance fee only accrues above the
    //     stored watermark, so a watermark that ever *decreased* would let the
    //     same gains be charged twice. `report_nav` must only ever ratchet it up.
    fuzz_assert_ge!(
        vault.high_watermark,
        fixture.prev_high_watermark,
        "high_watermark regressed: {} < {}",
        vault.high_watermark,
        fixture.prev_high_watermark
    );
    fixture.prev_high_watermark = vault.high_watermark;

    // Note: `external_assets` (cost basis of invested base) and `total_assets`
    // (idle custody + the nav_authority's trusted mark) are independent — a
    // legitimate NAV markdown drops total_assets below external_assets without
    // any base leaving the system, so no ordering invariant holds between them.

    // 3. Withdrawal-queue accounting is backed by the live tickets. Every redeem
    //    adds its shares to `requested_withdrawal_shares` and opens a ticket;
    //    striking moves a ticket's value from shares into `assets_owed` /
    //    `pending_withdrawal_assets`; settling and cancelling close the ticket and
    //    unwind the counters. So at rest the vault's running totals must equal the
    //    sums over live tickets — catching dropped decrements that still conserve
    //    base (e.g. forgetting to reduce pending, or mis-accounting requested
    //    shares), which the conservation check alone cannot see.
    let tickets = fixture.live_tickets();
    let requested_shares: u128 = tickets
        .iter()
        .filter(|(_, _, _, t)| t.assets_owed == 0)
        .map(|(_, _, _, t)| t.shares_burned as u128)
        .sum();
    let pending_assets: u128 = tickets
        .iter()
        .map(|(_, _, _, t)| t.assets_owed as u128)
        .sum();
    fuzz_assert_eq!(
        vault.requested_withdrawal_shares as u128,
        requested_shares,
        "requested_withdrawal_shares {} != sum of unstruck live tickets' shares_burned {}",
        vault.requested_withdrawal_shares,
        requested_shares
    );
    fuzz_assert_eq!(
        vault.pending_withdrawal_assets as u128,
        pending_assets,
        "pending_withdrawal_assets {} != sum of live tickets' assets_owed {}",
        vault.pending_withdrawal_assets,
        pending_assets
    );
}
