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
        program_config::ProgramConfig,
        sub_account::VaultSubAccount,
        vault::Vault,
        withdrawal_ticket::{WithdrawalTicket, WITHDRAWAL_STRIKE_DELAY_EPOCHS},
        Account as RoshiAccount,
    },
    ID,
};
use roshi_interface::{
    access::{access_merkle_leaf, access_merkle_node, verify_access_merkle_proof},
    find_share_mint_address,
    math::{assets_for_redeem, shares_for_deposit},
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
    mint_supply, pyth_price_data, set_ata, set_ata_with_program, set_extended_token_2022_mint,
    set_mint, set_pyth_price, set_token_2022_mint, set_token_account,
    set_token_account_with_program, token_balance,
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
    /// Program authority + every vault role + fee payer (shared keypair: the
    /// accounting invariants don't gain from split authorities).
    operator: Rc<Keypair>,
    external_authority: Rc<Keypair>,
    vault: Pubkey,
    share_mint: Pubkey,
    base_mint: Pubkey,
    treasury: Pubkey,
    sub_account: Pubkey,
    custody: Pubkey,
    external_account: Pubkey,
    /// Pre-authorized Manager action: an SPL token transfer custody -> external
    /// signed by the sub-account PDA, with the amount left free. Drives the
    /// arbitrary-CPI authorization machinery (`manage`, `validate_authorized_cpi`,
    /// `invoke_authorized_cpi`, the custody clean-check) through real CPI.
    manage_action: Pubkey,
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
    extended_token_2022_mint: Pubkey,
    extended_token_2022_asset_pda: Pubkey,
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

#[fuzz_fixture]
impl RoshiFixture {
    pub fn setup() -> Self {
        let mut ctx = TestContext::new();
        let program_id = ID;
        ctx.add_program(&program_id, "../target/deploy/roshi.so")
            .expect("load roshi.so (run `just build` first)");

        let operator = Rc::new(Keypair::new());
        let external_authority = Rc::new(Keypair::new());
        ctx.svm.airdrop(&operator.pubkey(), FUND_LAMPORTS).unwrap();
        ctx.svm
            .airdrop(&external_authority.pubkey(), FUND_LAMPORTS)
            .unwrap();

        // 1. Program config.
        let (config_pda, _) = ProgramConfig::find_address();
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_program(
                operator.pubkey(),
                config_pda,
                operator.pubkey(),
            )
            .unwrap(),
            &[&operator],
            "initialize_program",
        );

        // 2. Vault. Single custody (deposit == withdraw sub-account) so the
        //    deposit -> redeem -> process_withdrawals loop is self-contained
        //    without a strategist rebalance (CPI; deferred to phase 2).
        let base_mint = Pubkey::new_unique();
        let (vault, bump) = Vault::find_address(b"main", &base_mint).unwrap();
        let share_mint = find_share_mint_address(&vault).0;
        let treasury = Pubkey::new_unique();
        let op = operator.pubkey().to_bytes();

        set_mint(&mut ctx.svm, base_mint, &vault, BASE_DECIMALS);
        set_token_account(&mut ctx.svm, treasury, &base_mint, &Pubkey::new_unique(), 0);

        let args = InitializeVaultArgs {
            tag: pad_tag(b"main"),
            tag_len: 4,
            admin: op,
            strategist: op,
            swap_authority: op,
            nav_authority: op,
            withdrawal_authority: op,
            base_mint: base_mint.to_bytes(),
            base_decimals: BASE_DECIMALS,
            base_oracle: OracleConfig::default(),
            deposit_sub_account: 0,
            withdraw_sub_account: 0,
            treasury: treasury.to_bytes(),
            performance_fee_bps: PERF_FEE_BPS,
            withdrawal_buffer_bps: WITHDRAWAL_BUFFER_BPS,
            private: false,
            access_merkle_root: [0; 32],
        };
        let _ = bump;
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_vault(
                operator.pubkey(),
                config_pda,
                operator.pubkey(),
                vault,
                args,
            )
            .unwrap(),
            &[&operator],
            "initialize_vault",
        );

        // 3. Enable external investing.
        submit_ok(
            &mut ctx,
            roshi_client::instruction::update_vault_config(
                operator.pubkey(),
                vault,
                UpdateVaultConfigArgs {
                    treasury: treasury.to_bytes(),
                    deposit_sub_account: 0,
                    withdraw_sub_account: 0,
                    base_oracle: OracleConfig::default(),
                    performance_fee_bps: PERF_FEE_BPS,
                    withdrawal_buffer_bps: WITHDRAWAL_BUFFER_BPS,
                    external_enabled: true,
                },
            )
            .unwrap(),
            &[&operator],
            "update_vault_config",
        );

        // 4. Custody + external token accounts (base).
        let sub_account = VaultSubAccount::find_address(&vault, 0).0;
        let custody = set_ata(&mut ctx.svm, &sub_account, &base_mint, 0);
        let external_account = set_ata(&mut ctx.svm, &external_authority.pubkey(), &base_mint, 0);

        // 4b. Authorize one Manager action: an SPL token transfer custody ->
        //     external, signed by the sub-account PDA, amount free. The
        //     recomputed hash at `manage` time must match this — the authz path
        //     under test.
        let (manage_action, _) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            custody,
            external_account,
            ActionScope::Manager,
        );

        // 4c. Second base custody (the swap output leg) owned by the sub-account,
        //     plus a Swap action in each direction between it and the deposit
        //     custody. Drives `swap` end to end.
        let swap_custody = Pubkey::new_unique();
        set_token_account(&mut ctx.svm, swap_custody, &base_mint, &sub_account, 0);
        let (swap_forward_action, _) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            custody,
            swap_custody,
            ActionScope::Swap,
        );
        let (swap_reverse_action, _) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            swap_custody,
            custody,
            ActionScope::Swap,
        );

        // 4d. AtomicRedeem: a sub-account-owned venue account pre-funded with
        //     deployed capital, plus an AtomicRedeem action whose unwind CPI
        //     pulls base venue -> custody. Empty ops authorize any CPI to the
        //     token program; the redeem is bounded by the on-chain share
        //     entitlement and the requirement that custody only ever increases
        //     across the CPI. `redeem_amount_offset = 1` is where the transfer
        //     amount sits in the token-transfer ix data ([tag, amount_le]).
        let atomic_venue = Pubkey::new_unique();
        set_token_account(&mut ctx.svm, atomic_venue, &base_mint, &sub_account, VENUE_BASE);
        let atomic_action_hash =
            compute_action_hash_from_metas(&support::TOKEN_PROGRAM_ID, &Ops::empty(), &[], &[])
                .expect("action hash");
        let (atomic_action, _) = Action::find_address(&vault, &atomic_action_hash);
        submit_ok(
            &mut ctx,
            roshi_client::instruction::authorize_action(
                operator.pubkey(),
                vault,
                atomic_action,
                atomic_action_hash,
                ActionScope::AtomicRedeem,
                Ops::empty(),
                1,
            )
            .unwrap(),
            &[&operator],
            "authorize_action(atomic_redeem)",
        );

        // 4e. A revocable Manager action (custody -> treasury) used only to drive
        //     `revoke_action`: action_revoke_action closes it and asserts a manage
        //     against it then moves no funds, then re-authorizes it. Distinct
        //     destination from manage_action so it gets its own Action PDA.
        let (revocable_action, revocable_action_hash) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            custody,
            treasury,
            ActionScope::Manager,
        );

        // 4f. Register a non-base asset priced through a mock Pyth feed. The
        //     custody is the sub-account's ATA for the asset mint; the price
        //     account is installed fresh (publish_time == now == 0) so deposits
        //     price through `oracle.rs` from the first action. This exercises
        //     `initialize_asset` for real (admin-signed, PDA-funded).
        let asset_mint = Pubkey::new_unique();
        set_mint(&mut ctx.svm, asset_mint, &operator.pubkey(), ASSET_DECIMALS);
        let asset_custody = set_ata(&mut ctx.svm, &sub_account, &asset_mint, 0);
        let pyth_account = Pubkey::new_unique();
        set_pyth_price(
            &mut ctx.svm,
            pyth_account,
            PYTH_FEED_ID,
            PYTH_BASE_PRICE,
            0,
            PYTH_EXPONENT,
            0,
        );
        let (asset_pda, _) = Asset::find_address(&vault, &asset_mint);
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_asset(
                operator.pubkey(),
                vault,
                asset_mint,
                asset_pda,
                InitializeAssetArgs {
                    asset_mint: asset_mint.to_bytes(),
                    oracle: OracleConfig::pyth(PythOracleConfig::new(
                        PYTH_FEED_ID,
                        PYTH_PRICE_DECIMALS,
                        PYTH_MAX_AGE_SECS,
                        PYTH_MAX_CONF_BPS,
                    )),
                    asset_decimals: ASSET_DECIMALS,
                    enabled: true,
                },
            )
            .unwrap(),
            &[&operator],
            "initialize_asset",
        );

        // 4g. Register a bare Token-2022 asset. Extended Token-2022 mints are
        //     installed too but intentionally not registered; an action below
        //     asserts `initialize_asset` rejects them before creating the PDA.
        let token_2022_asset_mint = Pubkey::new_unique();
        set_token_2022_mint(
            &mut ctx.svm,
            token_2022_asset_mint,
            &operator.pubkey(),
            ASSET_DECIMALS,
        );
        let token_2022_asset_custody = set_ata_with_program(
            &mut ctx.svm,
            &sub_account,
            &token_2022_asset_mint,
            0,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let token_2022_swap_custody = Pubkey::new_unique();
        set_token_account_with_program(
            &mut ctx.svm,
            token_2022_swap_custody,
            &token_2022_asset_mint,
            &sub_account,
            0,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let (token_2022_swap_forward_action, _) = authorize_transfer_action_with_program(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            token_2022_asset_custody,
            token_2022_swap_custody,
            ActionScope::Swap,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let (token_2022_swap_reverse_action, _) = authorize_transfer_action_with_program(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            token_2022_swap_custody,
            token_2022_asset_custody,
            ActionScope::Swap,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let (token_2022_asset_pda, _) = Asset::find_address(&vault, &token_2022_asset_mint);
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_asset(
                operator.pubkey(),
                vault,
                token_2022_asset_mint,
                token_2022_asset_pda,
                InitializeAssetArgs {
                    asset_mint: token_2022_asset_mint.to_bytes(),
                    oracle: OracleConfig::pyth(PythOracleConfig::new(
                        PYTH_FEED_ID,
                        PYTH_PRICE_DECIMALS,
                        PYTH_MAX_AGE_SECS,
                        PYTH_MAX_CONF_BPS,
                    )),
                    asset_decimals: ASSET_DECIMALS,
                    enabled: true,
                },
            )
            .unwrap(),
            &[&operator],
            "initialize_asset(token_2022)",
        );

        let extended_token_2022_mint = Pubkey::new_unique();
        set_extended_token_2022_mint(
            &mut ctx.svm,
            extended_token_2022_mint,
            &operator.pubkey(),
            ASSET_DECIMALS,
        );
        let (extended_token_2022_asset_pda, _) =
            Asset::find_address(&vault, &extended_token_2022_mint);

        // 5. Users, each funded with base + the non-base asset; share ATA
        //    starts empty.
        let mut users = Vec::with_capacity(NUM_USERS);
        let mut base_accounts = vec![custody, swap_custody, atomic_venue, external_account, treasury];
        let mut asset_accounts = vec![asset_custody];
        let mut token_2022_asset_accounts = vec![token_2022_asset_custody, token_2022_swap_custody];
        for _ in 0..NUM_USERS {
            let kp = Rc::new(Keypair::new());
            ctx.svm.airdrop(&kp.pubkey(), FUND_LAMPORTS).unwrap();
            let base_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &base_mint, INITIAL_USER_BASE);
            let share_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &share_mint, 0);
            let asset_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &asset_mint, INITIAL_USER_ASSET);
            let token_2022_asset_ata = set_ata_with_program(
                &mut ctx.svm,
                &kp.pubkey(),
                &token_2022_asset_mint,
                INITIAL_USER_ASSET,
                support::TOKEN_2022_PROGRAM_ID,
            );
            base_accounts.push(base_ata);
            asset_accounts.push(asset_ata);
            token_2022_asset_accounts.push(token_2022_asset_ata);
            users.push(FuzzUser {
                kp,
                base_ata,
                share_ata,
                asset_ata,
                token_2022_asset_ata,
                access_proof: Vec::new(),
            });
        }

        // 6. Whitelist every user in a real access tree and flip the vault
        //    private. Members deposit with their proofs; the core loop survives.
        let leaves: Vec<[u8; 32]> = users
            .iter()
            .map(|u| access_merkle_leaf(&u.kp.pubkey()))
            .collect();
        let (members_root, proofs) = build_access_tree(&leaves);
        for (user, proof) in users.iter_mut().zip(proofs) {
            // Fail loudly at setup if the builder and the program's verifier
            // disagree, rather than silently breaking every member deposit.
            assert!(
                verify_access_merkle_proof(&user.kp.pubkey(), &members_root, &proof),
                "access tree builder produced an invalid member proof"
            );
            user.access_proof = proof;
        }

        // An outsider absent from the tree, carrying a stolen member proof.
        let outsider_kp = Rc::new(Keypair::new());
        ctx.svm.airdrop(&outsider_kp.pubkey(), FUND_LAMPORTS).unwrap();
        let outsider_base = set_ata(&mut ctx.svm, &outsider_kp.pubkey(), &base_mint, INITIAL_USER_BASE);
        let outsider_share = set_ata(&mut ctx.svm, &outsider_kp.pubkey(), &share_mint, 0);
        let outsider_asset = set_ata(&mut ctx.svm, &outsider_kp.pubkey(), &asset_mint, 0);
        let outsider_token_2022_asset = set_ata_with_program(
            &mut ctx.svm,
            &outsider_kp.pubkey(),
            &token_2022_asset_mint,
            0,
            support::TOKEN_2022_PROGRAM_ID,
        );
        base_accounts.push(outsider_base);
        asset_accounts.push(outsider_asset);
        token_2022_asset_accounts.push(outsider_token_2022_asset);
        let outsider = FuzzUser {
            kp: outsider_kp,
            base_ata: outsider_base,
            share_ata: outsider_share,
            asset_ata: outsider_asset,
            token_2022_asset_ata: outsider_token_2022_asset,
            access_proof: users[0].access_proof.clone(),
        };

        submit_ok(
            &mut ctx,
            roshi_client::instruction::set_vault_access(operator.pubkey(), vault, true, members_root)
                .unwrap(),
            &[&operator],
            "set_vault_access",
        );

        let initial_base =
            (NUM_USERS + 1) as u128 * INITIAL_USER_BASE as u128 + VENUE_BASE as u128;
        let initial_asset = NUM_USERS as u128 * INITIAL_USER_ASSET as u128;
        let initial_token_2022_asset = NUM_USERS as u128 * INITIAL_USER_ASSET as u128;

        Self {
            ctx,
            program_id,
            operator,
            external_authority,
            vault,
            share_mint,
            base_mint,
            treasury,
            sub_account,
            custody,
            external_account,
            manage_action,
            swap_custody,
            swap_forward_action,
            swap_reverse_action,
            atomic_venue,
            atomic_action,
            revocable_action,
            revocable_action_hash,
            members_root,
            outsider,
            users,
            base_accounts,
            initial_base,
            asset_mint,
            asset_pda,
            asset_custody,
            pyth_account,
            asset_accounts,
            initial_asset,
            token_2022_asset_mint,
            token_2022_asset_pda,
            token_2022_asset_custody,
            token_2022_swap_custody,
            token_2022_swap_forward_action,
            token_2022_swap_reverse_action,
            extended_token_2022_mint,
            extended_token_2022_asset_pda,
            token_2022_asset_accounts,
            initial_token_2022_asset,
            report_nonce: 0,
            prev_high_watermark: 0,
        }
    }

    /// Pull base into custody and mint shares. The user is whitelisted, so its
    /// access proof verifies whether the vault is private or public.
    pub fn action_deposit(&mut self, #[range(0..NUM_USERS)] user: usize, amount: u64) -> bool {
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.base_ata);
        if balance == 0 {
            return false;
        }
        // [0, balance]: keeps the action mostly valid for reachability while still
        // hitting the zero-amount and exact-balance (full-drain) boundaries.
        let amount = amount % (balance + 1);
        let ix = roshi_client::instruction::deposit(
            user.kp.pubkey(),
            self.vault,
            user.base_ata,
            self.custody,
            user.share_ata,
            self.share_mint,
            support::TOKEN_PROGRAM_ID,
            self.base_mint,
            amount,
            0,
            user.access_proof.clone(),
            vec![],
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&user.kp])
    }

    /// Build a deposit of the registered non-base asset: source is the user's
    /// asset ATA, custody is the asset custody, and the asset PDA + Pyth price
    /// account ride as extra metas so the program prices asset -> base atoms
    /// through the oracle. Shared by the organic action and the oracle
    /// negatives, which differ only in the installed price.
    fn deposit_asset_ix(&self, user: &FuzzUser, amount: u64) -> solana_instruction::Instruction {
        roshi_client::instruction::deposit(
            user.kp.pubkey(),
            self.vault,
            user.asset_ata,
            self.asset_custody,
            user.share_ata,
            self.share_mint,
            support::TOKEN_PROGRAM_ID,
            self.asset_mint,
            amount,
            0,
            user.access_proof.clone(),
            vec![
                AccountMeta::new_readonly(self.asset_pda, false),
                AccountMeta::new_readonly(self.pyth_account, false),
            ],
        )
        .unwrap()
    }

    fn deposit_token_2022_asset_ix(
        &self,
        user: &FuzzUser,
        amount: u64,
    ) -> solana_instruction::Instruction {
        roshi_client::instruction::deposit(
            user.kp.pubkey(),
            self.vault,
            user.token_2022_asset_ata,
            self.token_2022_asset_custody,
            user.share_ata,
            self.share_mint,
            support::TOKEN_2022_PROGRAM_ID,
            self.token_2022_asset_mint,
            amount,
            0,
            user.access_proof.clone(),
            vec![
                AccountMeta::new_readonly(self.token_2022_asset_pda, false),
                AccountMeta::new_readonly(self.pyth_account, false),
            ],
        )
        .unwrap()
    }

    fn unix_timestamp(&self) -> i64 {
        let clock: Clock = self.ctx.svm.get_sysvar();
        clock.unix_timestamp
    }

    fn asset_enabled(&self) -> bool {
        let account = self.ctx.get_account(&self.asset_pda).expect("asset exists");
        match wincode::deserialize::<RoshiAccount>(&account.data) {
            Ok(RoshiAccount::Asset(asset)) => asset.enabled().expect("asset flag decodes"),
            Ok(_) => panic!("asset PDA is not an Asset account"),
            Err(_) => panic!("asset PDA failed to deserialize"),
        }
    }

    fn fresh_asset_deposit_can_reach_transfer(&self, vault: &Vault, amount: u64) -> bool {
        let Ok(economic_share_supply) =
            vault.economic_share_supply(mint_supply(&self.ctx.svm, &self.share_mint))
        else {
            return false;
        };
        let Some(base_atoms) = amount.checked_mul(2) else {
            return false;
        };
        shares_for_deposit(
            base_atoms,
            vault.total_assets,
            economic_share_supply,
            BASE_DECIMALS,
        )
        .is_ok()
    }

    /// Rewrite the Pyth account through `TestContext::write_account`, so
    /// Crucible's per-iteration snapshot/dirty-account machinery observes the
    /// mutation. Direct `svm.set_account` is setup-only.
    fn write_pyth_price(&mut self, conf: u64, publish_time: i64) {
        let data = pyth_price_data(
            PYTH_FEED_ID,
            PYTH_BASE_PRICE,
            conf,
            PYTH_EXPONENT,
            publish_time,
        );
        let lamports = self
            .ctx
            .get_account(&self.pyth_account)
            .map(|a| a.lamports)
            .unwrap_or_else(|_| self.ctx.svm.minimum_balance_for_rent_exemption(data.len()));
        self.ctx
            .write_account(
                &self.pyth_account,
                Account {
                    lamports,
                    data,
                    owner: support::PYTH_RECEIVER_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .expect("write pyth account");
    }

    /// Refresh the price to the current clock timestamp and prove a real
    /// non-base deposit can pass the oracle path when deposits and the asset are
    /// enabled.
    pub fn action_deposit_asset_fresh_price(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        self.write_pyth_price(0, self.unix_timestamp());
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.asset_ata);
        if balance == 0 {
            return false;
        }
        let amount = (amount % balance) + 1;
        let vault = self.load_vault();
        let assert_oracle_ok = !vault.deposits_paused().unwrap_or(true)
            && self.asset_enabled()
            && vault.total_assets == 0
            && mint_supply(&self.ctx.svm, &self.share_mint) == 0;
        let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
        let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
        let ix = self.deposit_asset_ix(&user, amount);
        let ok = submit(&mut self.ctx, ix, &[&user.kp]);
        let source_after = token_balance(&self.ctx.svm, &user.asset_ata);
        let custody_after = token_balance(&self.ctx.svm, &self.asset_custody);
        if assert_oracle_ok {
            fuzz_assert!(
                ok && source_after == source_before - amount
                    && custody_after == custody_before + amount,
                "fresh Pyth price rejected or moved wrong asset amount: \
                 ok={ok}, source {source_before}->{source_after}, custody {custody_before}->{custody_after}, amount={amount}"
            );
        }
        ok
    }

    /// Install a stale Pyth update and assert a positive asset deposit rejects
    /// without moving tokens whenever execution reaches the oracle gate.
    pub fn action_deposit_asset_stale_price(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        self.write_pyth_price(0, self.unix_timestamp() - PYTH_MAX_AGE_SECS as i64 - 1);
        self.assert_asset_deposit_rejects(user, amount, "stale Pyth price")
    }

    /// Install an over-wide confidence interval and assert the configured
    /// `max_confidence_bps` guard rejects the deposit without moving tokens.
    pub fn action_deposit_asset_wide_confidence(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        self.write_pyth_price(PYTH_BASE_PRICE as u64, self.unix_timestamp());
        self.assert_asset_deposit_rejects(user, amount, "wide Pyth confidence")
    }

    fn assert_asset_deposit_rejects(&mut self, user: usize, amount: u64, reason: &str) -> bool {
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.asset_ata);
        if balance == 0 {
            return false;
        }
        let amount = (amount % balance) + 1;
        let vault = self.load_vault();
        let assert_reject = !vault.deposits_paused().unwrap_or(true)
            && self.asset_enabled()
            && self.fresh_asset_deposit_can_reach_transfer(&vault, amount);
        let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
        let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
        let ix = self.deposit_asset_ix(&user, amount);
        let ok = submit(&mut self.ctx, ix, &[&user.kp]);
        let source_after = token_balance(&self.ctx.svm, &user.asset_ata);
        let custody_after = token_balance(&self.ctx.svm, &self.asset_custody);
        if assert_reject {
            fuzz_assert!(
                !ok && source_after == source_before && custody_after == custody_before,
                "asset deposit admitted despite {reason}: \
                 ok={ok}, source {source_before}->{source_after}, custody {custody_before}->{custody_after}, amount={amount}"
            );
        }
        ok
    }

    /// Deposit the registered non-base asset. The program prices asset atoms
    /// into base terms via the Pyth oracle (staleness + confidence checked),
    /// credits `total_assets`, and the asset tokens land in the asset custody.
    pub fn action_deposit_asset(&mut self, #[range(0..NUM_USERS)] user: usize, amount: u64) -> bool {
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.asset_ata);
        if balance == 0 {
            return false;
        }
        // [0, balance]: mostly valid, still hits zero-amount and full-drain.
        let amount = amount % (balance + 1);
        let ix = self.deposit_asset_ix(&user, amount);
        submit(&mut self.ctx, ix, &[&user.kp])
    }

    /// Deposit the registered bare Token-2022 asset through the Token-2022
    /// program id. In a clean first-deposit state, assert that a fresh oracle
    /// lets real Token-2022 atoms move into custody; elsewhere the action still
    /// explores the path without over-claiming every later NAV state must admit
    /// the deposit.
    pub fn action_deposit_token_2022_asset(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        self.write_pyth_price(0, self.unix_timestamp());
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.token_2022_asset_ata);
        if balance == 0 {
            return false;
        }
        let amount = (amount % balance) + 1;
        let vault = self.load_vault();
        let assert_token_2022_ok = !vault.deposits_paused().unwrap_or(true)
            && vault.total_assets == 0
            && mint_supply(&self.ctx.svm, &self.share_mint) == 0;
        let source_before = token_balance(&self.ctx.svm, &user.token_2022_asset_ata);
        let custody_before = token_balance(&self.ctx.svm, &self.token_2022_asset_custody);
        let ix = self.deposit_token_2022_asset_ix(&user, amount);
        let ok = submit(&mut self.ctx, ix, &[&user.kp]);
        let source_after = token_balance(&self.ctx.svm, &user.token_2022_asset_ata);
        let custody_after = token_balance(&self.ctx.svm, &self.token_2022_asset_custody);
        if assert_token_2022_ok {
            fuzz_assert!(
                ok && source_after == source_before - amount
                    && custody_after == custody_before + amount,
                "Token-2022 asset deposit rejected or moved wrong amount: \
                 ok={ok}, source {source_before}->{source_after}, custody {custody_before}->{custody_after}, amount={amount}"
            );
        }
        ok
    }

    /// Extended Token-2022 mints must be rejected by `initialize_asset` before
    /// the Asset PDA is created. Bare 82-byte Token-2022 mints are covered by
    /// setup and `action_deposit_token_2022_asset`; this pins the opposite edge.
    pub fn action_initialize_extended_token_2022_asset_rejects(&mut self) -> bool {
        let ix = roshi_client::instruction::initialize_asset(
            self.operator.pubkey(),
            self.vault,
            self.extended_token_2022_mint,
            self.extended_token_2022_asset_pda,
            InitializeAssetArgs {
                asset_mint: self.extended_token_2022_mint.to_bytes(),
                oracle: OracleConfig::pyth(PythOracleConfig::new(
                    PYTH_FEED_ID,
                    PYTH_PRICE_DECIMALS,
                    PYTH_MAX_AGE_SECS,
                    PYTH_MAX_CONF_BPS,
                )),
                asset_decimals: ASSET_DECIMALS,
                enabled: true,
            },
        )
        .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        let created = self
            .ctx
            .get_account(&self.extended_token_2022_asset_pda)
            .is_ok();
        fuzz_assert!(
            !ok && !created,
            "extended Token-2022 mint initialized as asset: success={ok}, created={created}"
        );
        ok
    }

    /// Drive `update_asset`: disable the registered asset, assert a positive
    /// deposit is blocked without token movement, then re-enable it so later
    /// asset/oracle paths remain reachable in the same sequence.
    pub fn action_update_asset_disable_rejects(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        let oracle = OracleConfig::pyth(PythOracleConfig::new(
            PYTH_FEED_ID,
            PYTH_PRICE_DECIMALS,
            PYTH_MAX_AGE_SECS,
            PYTH_MAX_CONF_BPS,
        ));
        let disable = roshi_client::instruction::update_asset(
            self.operator.pubkey(),
            self.vault,
            self.asset_pda,
            UpdateAssetArgs {
                oracle,
                enabled: false,
            },
        )
        .unwrap();
        if !submit(&mut self.ctx, disable, &[&self.operator.clone()]) {
            return false;
        }

        self.write_pyth_price(0, self.unix_timestamp());
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.asset_ata);
        let vault = self.load_vault();
        if balance > 0 && !vault.deposits_paused().unwrap_or(true) {
            let amount = (amount % balance) + 1;
            let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
            let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
            let ix = self.deposit_asset_ix(&user, amount);
            let ok = submit(&mut self.ctx, ix, &[&user.kp]);
            let source_after = token_balance(&self.ctx.svm, &user.asset_ata);
            let custody_after = token_balance(&self.ctx.svm, &self.asset_custody);
            fuzz_assert!(
                !ok && source_after == source_before && custody_after == custody_before,
                "disabled asset accepted deposit: \
                 ok={ok}, source {source_before}->{source_after}, custody {custody_before}->{custody_after}, amount={amount}"
            );
        }

        let enable = roshi_client::instruction::update_asset(
            self.operator.pubkey(),
            self.vault,
            self.asset_pda,
            UpdateAssetArgs {
                oracle,
                enabled: true,
            },
        )
        .unwrap();
        submit(&mut self.ctx, enable, &[&self.operator.clone()])
    }

    /// Attempt a deposit from the non-whitelisted outsider (with a stolen member
    /// proof). The access-control property: while the vault is private it must be
    /// rejected and mint no shares; when public it deposits like anyone else.
    /// Conservation can't see a leak here (the outsider's accounts are tracked),
    /// so assert the private-state rejection directly.
    pub fn action_deposit_outsider(&mut self, amount: u64) -> bool {
        let outsider = self.outsider.clone();
        let balance = token_balance(&self.ctx.svm, &outsider.base_ata);
        if balance == 0 {
            return false;
        }
        // 1..=balance: a real deposit attempt, so an erroneous accept is visible.
        let amount = (amount % balance) + 1;
        // The access check only runs when the vault is private AND deposits are
        // enabled — `try_deposit` checks the pause gate first, so asserting
        // rejection while paused would prove only the pause, not the ACL.
        let vault = self.load_vault();
        let assert_acl = vault.private().unwrap_or(false) && !vault.deposits_paused().unwrap_or(true);
        let shares_before = token_balance(&self.ctx.svm, &outsider.share_ata);
        let ix = roshi_client::instruction::deposit(
            outsider.kp.pubkey(),
            self.vault,
            outsider.base_ata,
            self.custody,
            outsider.share_ata,
            self.share_mint,
            support::TOKEN_PROGRAM_ID,
            self.base_mint,
            amount,
            0,
            outsider.access_proof.clone(),
            vec![],
        )
        .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&outsider.kp]);
        let shares_after = token_balance(&self.ctx.svm, &outsider.share_ata);
        if assert_acl {
            fuzz_assert!(
                !ok && shares_after == shares_before,
                "non-whitelisted deposit admitted to a private vault: \
                 shares {shares_before} -> {shares_after} (success={ok})"
            );
        }
        ok
    }

    /// Toggle the vault's access mode. Private always uses `members_root` (so
    /// member proofs stay valid and the core loop survives); public uses the
    /// empty root. Drives `set_vault_access` and both `allows_depositor` branches.
    pub fn action_set_vault_access(&mut self, make_private: bool) -> bool {
        let root = if make_private {
            self.members_root
        } else {
            [0; 32]
        };
        let ix = roshi_client::instruction::set_vault_access(
            self.operator.pubkey(),
            self.vault,
            make_private,
            root,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Decode the current vault state from on-chain data.
    fn load_vault(&self) -> Vault {
        let account = self.ctx.get_account(&self.vault).expect("vault exists");
        Vault::from_account_data(&account.data).expect("vault decodes")
    }

    /// Burn shares and queue a withdrawal ticket.
    pub fn action_redeem(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        #[range(0..TICKETS_PER_USER)] ticket_index: u8,
        shares: u64,
    ) -> bool {
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.share_ata);
        if balance == 0 {
            return false;
        }
        let shares = shares % (balance + 1);
        let ticket =
            WithdrawalTicket::find_address(&self.vault, &user.kp.pubkey(), ticket_index).0;
        let ix = roshi_client::instruction::redeem(
            user.kp.pubkey(),
            self.vault,
            user.share_ata,
            self.share_mint,
            user.base_ata,
            ticket,
            ticket_index,
            shares,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&user.kp])
    }

    /// Unwind a queued ticket, returning the shares to the owner.
    pub fn action_cancel_redeem(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        #[range(0..TICKETS_PER_USER)] ticket_index: u8,
    ) -> bool {
        let user = self.users[user].clone();
        let ticket =
            WithdrawalTicket::find_address(&self.vault, &user.kp.pubkey(), ticket_index).0;
        let ix = roshi_client::instruction::cancel_redeem(
            user.kp.pubkey(),
            self.vault,
            ticket,
            self.share_mint,
            user.share_ata,
            0,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&user.kp])
    }

    /// Settle every *settleable* ticket in one batch, paying base from withdraw
    /// custody to each recipient. Batching the ready tickets (rather than poking
    /// one random index) is how a real withdrawal keeper works, and it lets the
    /// deep `deposit -> redeem -> report_nav -> process` chain actually fire:
    /// targeting a single random (user, ticket) almost never hits a live ticket,
    /// and the miss adds no new coverage for the fuzzer to learn from.
    pub fn action_process_withdrawals(&mut self) -> bool {
        let settlements = self.settleable_tickets();
        if settlements.is_empty() {
            return false;
        }
        let ix = roshi_client::instruction::process_withdrawals(
            self.operator.pubkey(),
            self.vault,
            self.sub_account,
            self.custody,
            self.share_mint,
            settlements,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Current NAV report epoch. The vault is created in `setup()` and never
    /// closed, so a read failure is a harness bug — fail loudly.
    fn report_epoch(&self) -> u64 {
        let account = self
            .ctx
            .get_account(&self.vault)
            .expect("vault account must exist");
        Vault::from_account_data(&account.data)
            .expect("vault must deserialize")
            .report_epoch
    }

    /// Every live withdrawal ticket as `(ticket, owner, destination, state)`.
    /// Drives both settlement and the ticket-accounting invariants. A *missing*
    /// PDA means no live ticket (settled/cancelled tickets are closed to `None`);
    /// but a PDA that is *present* must decode as a `WithdrawalTicket` — anything
    /// else is a program/harness bug we must not silently skip, since skipping it
    /// would let a malformed-ticket accounting bug pass every invariant.
    fn live_tickets(&self) -> Vec<(Pubkey, Pubkey, Pubkey, WithdrawalTicket)> {
        let mut out = Vec::new();
        for u in &self.users {
            let (owner, dest) = (u.kp.pubkey(), u.base_ata);
            for ti in 0..TICKETS_PER_USER {
                let ticket = WithdrawalTicket::find_address(&self.vault, &owner, ti).0;
                let Ok(account) = self.ctx.get_account(&ticket) else {
                    continue; // closed / never opened
                };
                match wincode::deserialize::<RoshiAccount>(&account.data) {
                    Ok(RoshiAccount::WithdrawalTicket(t)) => out.push((ticket, owner, dest, t)),
                    Ok(_) => panic!("account at ticket PDA {ticket} is not a WithdrawalTicket"),
                    Err(_) => panic!(
                        "ticket PDA {ticket} present ({}B) but failed to deserialize",
                        account.data.len()
                    ),
                }
            }
        }
        out
    }

    /// `(ticket, owner, destination)` for every ticket `process_withdrawals` can
    /// settle now: already priced, or strikable this epoch. Mirrors the handler's
    /// `strike_ticket` gate exactly (`report_epoch >= request_epoch +
    /// WITHDRAWAL_STRIKE_DELAY_EPOCHS`, with `checked_add` so a `u64::MAX` epoch
    /// is treated as not-yet-strikable, as the program would). Not-yet-strikable
    /// unpriced tickets are excluded so they don't fail the whole batch.
    fn settleable_tickets(&self) -> Vec<(Pubkey, Pubkey, Pubkey)> {
        let report_epoch = self.report_epoch();
        self.live_tickets()
            .into_iter()
            .filter(|(_, _, _, t)| {
                let strikable = t
                    .request_epoch
                    .checked_add(WITHDRAWAL_STRIKE_DELAY_EPOCHS)
                    .is_some_and(|earliest| {
                        report_epoch >= earliest && t.request_epoch <= report_epoch
                    });
                t.assets_owed > 0 || strikable
            })
            .map(|(ticket, owner, dest, _)| (ticket, owner, dest))
            .collect()
    }

    /// Move idle custody base out to the external venue.
    pub fn action_invest_external(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let ix = roshi_client::instruction::invest_external(
            self.operator.pubkey(),
            self.vault,
            0,
            self.sub_account,
            self.custody,
            self.external_account,
            amount,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Return base from the external venue back into custody.
    pub fn action_return_external(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.external_account);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let ix = roshi_client::instruction::return_external(
            self.operator.pubkey(),
            self.external_authority.pubkey(),
            self.vault,
            0,
            self.sub_account,
            self.external_account,
            self.custody,
            amount,
        )
        .unwrap();
        let op = self.operator.clone();
        let ext = self.external_authority.clone();
        submit(&mut self.ctx, ix, &[&op, &ext])
    }

    /// Build a `manage` instruction that CPIs an SPL token transfer of `amount`
    /// from custody to `destination`, signed by the sub-account PDA, against the
    /// pre-authorized `action`. The recomputed action hash matches only when
    /// `(action, destination)` are a pinned pair (e.g. `manage_action` with
    /// `external_account`); any mismatch — wrong destination, or a revoked
    /// action whose account is closed — must reject.
    fn manage_transfer_ix(
        &self,
        action: Pubkey,
        destination: Pubkey,
        amount: u64,
    ) -> solana_instruction::Instruction {
        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&amount.to_le_bytes());
        roshi_client::instruction::manage(
            self.operator.pubkey(),
            self.vault,
            self.sub_account,
            action,
            vec![
                AccountMeta::new(self.custody, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            ],
            ManageArgs {
                sub_account: 0,
                program_id: support::TOKEN_PROGRAM_ID.to_bytes(),
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

    /// Execute the authorized manager transfer (custody -> external) through the
    /// CPI authorization machinery. Conservation still holds — this just reaches
    /// the same custody/external move via `manage` rather than `invest_external`,
    /// exercising `validate_authorized_cpi` + `invoke_signed` with the real PDA.
    pub fn action_manage_transfer(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let ix = self.manage_transfer_ix(self.manage_action, self.external_account, amount);
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Reuse the authorized action PDA but swap the CPI destination to a user
    /// ATA the action never pinned. The recomputed hash must not match, so the
    /// program must reject it: no funds may leave custody. Conservation alone
    /// cannot see this (the user ATA is tracked), so assert it directly.
    pub fn action_manage_tampered_destination(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let destination = self.users[user].base_ata;
        let custody_before = token_balance(&self.ctx.svm, &self.custody);
        // 1..=available: a real transfer attempt, so a successful (buggy) move
        // would be observable rather than a no-op zero transfer.
        let amount = (amount % available) + 1;
        let ix = self.manage_transfer_ix(self.manage_action, destination, amount);
        let moved = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        let custody_after = token_balance(&self.ctx.svm, &self.custody);
        fuzz_assert!(
            !moved && custody_after == custody_before,
            "unauthorized manage moved custody funds to an unpinned destination: \
             custody {custody_before} -> {custody_after} (success={moved})"
        );
        moved
    }

    /// Drive `revoke_action` and its security guarantee. If the revocable Manager
    /// action is currently authorized, revoke it (admin signs), then attempt a
    /// `manage` against the now-closed action and assert it moves no custody
    /// funds — proving revocation removes authority (conservation can't see this:
    /// the would-be destination, treasury, is tracked). If it's already revoked,
    /// re-authorize it (same accounts → same hash/PDA) so the next call can
    /// revoke again.
    pub fn action_revoke_action(&mut self) -> bool {
        let authorized = self
            .ctx
            .get_account(&self.revocable_action)
            .map(|a| a.owner == self.program_id && !a.data.is_empty())
            .unwrap_or(false);

        if !authorized {
            let operator = self.operator.clone();
            let (action, _) = authorize_transfer_action(
                &mut self.ctx,
                &operator,
                self.vault,
                self.sub_account,
                self.custody,
                self.treasury,
                ActionScope::Manager,
            );
            debug_assert_eq!(action, self.revocable_action);
            return true;
        }

        let revoke = roshi_client::instruction::revoke_action(
            self.operator.pubkey(),
            self.vault,
            self.revocable_action,
            self.revocable_action_hash,
        )
        .unwrap();
        if !submit(&mut self.ctx, revoke, &[&self.operator.clone()]) {
            return false;
        }

        // The action is closed now: a manage against it must reject before any
        // transfer, leaving custody untouched. The check is only non-vacuous when
        // a *still-authorized* action could actually move funds — i.e. custody
        // holds at least the 1 atom we try to transfer and manage isn't paused.
        // Otherwise a broken revocation would be masked by insufficient-funds or
        // the pause gate, so skip the assertion (the revoke itself still ran).
        let custody_before = token_balance(&self.ctx.svm, &self.custody);
        if custody_before == 0 || self.load_vault().manage_paused().unwrap_or(true) {
            return true;
        }
        let ix = self.manage_transfer_ix(self.revocable_action, self.treasury, 1);
        let moved = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        let custody_after = token_balance(&self.ctx.svm, &self.custody);
        fuzz_assert!(
            !moved && custody_after == custody_before,
            "revoked action still moved custody funds: \
             {custody_before} -> {custody_after} (success={moved})"
        );
        true
    }

    /// Run two authorized custody -> external transfers in one `ManageBatch`.
    /// Both legs reuse the single authorized manage action (same accounts and
    /// discriminator hash to the same Action), so this exercises the batch
    /// loader's per-action `(sub_account, action)` pair loop and the
    /// per-action `accounts_start` slicing of the shared CPI account section.
    /// The second leg is sized to what the first leaves, so the batch settles.
    pub fn action_manage_batch(&mut self, amount_a: u64, amount_b: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let amount1 = amount_a % (available + 1);
        let remaining = available - amount1;
        let amount2 = amount_b % (remaining + 1);

        let mut ix_data_1 = vec![SPL_TRANSFER_TAG];
        ix_data_1.extend_from_slice(&amount1.to_le_bytes());
        let mut ix_data_2 = vec![SPL_TRANSFER_TAG];
        ix_data_2.extend_from_slice(&amount2.to_le_bytes());

        let pair = roshi_client::instruction::ManageBatchActionAccounts {
            sub_account_pda: self.sub_account,
            action: self.manage_action,
        };
        let transfer_flags = || {
            vec![
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
            ]
        };
        let leg = |start: u8, ix_data: Vec<u8>| ManageArgs {
            sub_account: 0,
            program_id: support::TOKEN_PROGRAM_ID.to_bytes(),
            accounts_start: start,
            accounts_len: 3,
            account_flags: transfer_flags(),
            ix_data,
        };
        // Shared CPI section: each leg's 3 metas immediately followed by its CPI
        // program account, so leg 0 selects [0,3) (program at 3) and leg 1
        // selects [4,7) (program at 7).
        let cpi_accounts = vec![
            AccountMeta::new(self.custody, false),
            AccountMeta::new(self.external_account, false),
            AccountMeta::new_readonly(self.sub_account, false),
            AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            AccountMeta::new(self.custody, false),
            AccountMeta::new(self.external_account, false),
            AccountMeta::new_readonly(self.sub_account, false),
            AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
        ];
        let ix = roshi_client::instruction::manage_batch(
            self.operator.pubkey(),
            self.vault,
            vec![pair, pair],
            cpi_accounts,
            vec![leg(0, ix_data_1), leg(4, ix_data_2)],
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Execute an authorized base->base swap between the two sub-account
    /// custodies. Degenerate as a swap, but exercises all of `try_swap`: the
    /// realized input/output bounds, custody reverification, and the signed CPI.
    /// `reverse` picks the direction so base is never one-way stranded.
    pub fn action_swap(&mut self, reverse: bool, amount: u64) -> bool {
        let (input, output, action) = if reverse {
            (self.swap_custody, self.custody, self.swap_reverse_action)
        } else {
            (self.custody, self.swap_custody, self.swap_forward_action)
        };
        let available = token_balance(&self.ctx.svm, &input);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&amount.to_le_bytes());
        let ix = roshi_client::instruction::swap(
            self.operator.pubkey(),
            self.vault,
            self.sub_account,
            input,
            output,
            action,
            vec![
                AccountMeta::new(input, false),
                AccountMeta::new(output, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            ],
            SwapArgs {
                // The transfer moves exactly `amount`, so spent == received ==
                // amount: within max_in and at/above min_out by construction.
                min_out: 0,
                max_in: amount,
                sub_account: 0,
                program_id: support::TOKEN_PROGRAM_ID.to_bytes(),
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
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Execute an authorized Token-2022 swap between two sub-account-owned
    /// custodies for the registered bare Token-2022 asset. This mirrors
    /// `action_swap`, but pins the CPI to the Token-2022 program id and asserts
    /// exact Token-2022 atom movement when managing is enabled.
    pub fn action_swap_token_2022_asset(&mut self, reverse: bool, amount: u64) -> bool {
        let (input, output, action) = if reverse {
            (
                self.token_2022_swap_custody,
                self.token_2022_asset_custody,
                self.token_2022_swap_reverse_action,
            )
        } else {
            (
                self.token_2022_asset_custody,
                self.token_2022_swap_custody,
                self.token_2022_swap_forward_action,
            )
        };
        let available = token_balance(&self.ctx.svm, &input);
        if available == 0 {
            return false;
        }
        let amount = (amount % available) + 1;
        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&amount.to_le_bytes());
        let input_before = token_balance(&self.ctx.svm, &input);
        let output_before = token_balance(&self.ctx.svm, &output);
        let should_succeed = !self.load_vault().manage_paused().unwrap_or(true);
        let ix = roshi_client::instruction::swap(
            self.operator.pubkey(),
            self.vault,
            self.sub_account,
            input,
            output,
            action,
            vec![
                AccountMeta::new(input, false),
                AccountMeta::new(output, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_2022_PROGRAM_ID, false),
            ],
            SwapArgs {
                min_out: amount,
                max_in: amount,
                sub_account: 0,
                program_id: support::TOKEN_2022_PROGRAM_ID.to_bytes(),
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
        .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        let input_after = token_balance(&self.ctx.svm, &input);
        let output_after = token_balance(&self.ctx.svm, &output);
        if should_succeed {
            fuzz_assert!(
                ok && input_after == input_before - amount
                    && output_after == output_before + amount,
                "Token-2022 swap rejected or moved wrong amount: \
                 ok={ok}, input {input_before}->{input_after}, output {output_before}->{output_after}, amount={amount}"
            );
        }
        ok
    }

    /// Redeem shares synchronously through the authorized unwind CPI: pull base
    /// from the venue into custody, pay the owner's recipient, and burn the
    /// shares. Exercises all of `try_atomic_redeem` — the share-balance and
    /// entitlement bounds, the unwind-into-custody check, payout, and burn. The
    /// unwind amount is sized to the on-chain entitlement (recomputed here with
    /// the same `assets_for_redeem`) and capped by the venue balance, so the
    /// redeem clears its own bounds.
    pub fn action_atomic_redeem(&mut self, #[range(0..NUM_USERS)] user: usize, shares: u64) -> bool {
        let user = self.users[user].clone();
        let share_balance = token_balance(&self.ctx.svm, &user.share_ata);
        if share_balance == 0 {
            return false;
        }
        let shares = (shares % share_balance) + 1;

        // Entitlement at the current NAV, recomputed exactly as the program does.
        let account = self.ctx.get_account(&self.vault).expect("vault exists");
        let vault = Vault::from_account_data(&account.data).expect("vault decodes");
        let supply = mint_supply(&self.ctx.svm, &self.share_mint);
        let Some(economic) = supply.checked_add(vault.requested_withdrawal_shares) else {
            return false;
        };
        let Ok(max_owed) = assets_for_redeem(shares, vault.total_assets, economic, BASE_DECIMALS)
        else {
            // Zero/invalid entitlement (e.g. nothing deposited yet): nothing to do.
            return false;
        };
        let unwind = max_owed.min(token_balance(&self.ctx.svm, &self.atomic_venue));
        if unwind == 0 {
            return false;
        }

        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&unwind.to_le_bytes());
        let ix = roshi_client::instruction::atomic_redeem(
            user.kp.pubkey(),
            self.vault,
            user.share_ata,
            self.share_mint,
            user.base_ata,
            self.custody,
            support::TOKEN_PROGRAM_ID,
            self.sub_account,
            self.atomic_action,
            vec![
                AccountMeta::new(self.atomic_venue, false),
                AccountMeta::new(self.custody, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            ],
            AtomicRedeemArgs {
                shares,
                min_output: 0,
                sub_account: 0,
                program_id: support::TOKEN_PROGRAM_ID.to_bytes(),
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
        .unwrap();
        submit(&mut self.ctx, ix, &[&user.kp])
    }

    /// Sweep accrued performance fees to the treasury.
    pub fn action_collect_fees(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        let amount = amount % (available + 1);
        let ix = roshi_client::instruction::collect_fees(
            self.operator.pubkey(),
            self.vault,
            0,
            self.sub_account,
            self.custody,
            self.treasury,
            amount,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Report NAV (advances the report epoch — which prices queued withdrawals —
    /// and accrues performance fees). The hash is always unique so the report
    /// isn't rejected as a replay; `external_value` is bounded to the system's
    /// base so NAV math stays in range and the report actually lands.
    pub fn action_report_nav(&mut self, #[range(0..4_000_000_000)] external_value: u64) -> bool {
        self.report_nonce += 1;
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&self.report_nonce.to_le_bytes());
        let ix = roshi_client::instruction::report_nav(
            self.operator.pubkey(),
            self.vault,
            self.share_mint,
            self.base_mint,
            self.custody,
            self.custody,
            external_value,
            hash,
        )
        .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        if ok {
            // NAV-report conservation: the program's own fee/liability arithmetic
            // must balance the moment a report lands. Gross NAV is idle custody +
            // the reported external value; out of it the program carves accrued
            // fees and pending withdrawals, leaving net `total_assets`. So
            //   total_assets + fees_payable + pending_withdrawal_assets
            //     == idle + external_value.
            // `report_nav` moves no tokens, so idle is unchanged from what the
            // program read. This pins the highest-risk subtraction in the program;
            // a stray over/under-charge of fees or liabilities breaks it even when
            // base conservation still holds. Single custody here (deposit ==
            // withdraw sub-account), so idle is the one custody balance.
            let account = self.ctx.get_account(&self.vault).expect("vault exists");
            let vault = Vault::from_account_data(&account.data).expect("vault decodes");
            let idle = token_balance(&self.ctx.svm, &self.custody) as u128;
            let net_plus_liabilities = vault.total_assets as u128
                + vault.fees_payable as u128
                + vault.pending_withdrawal_assets as u128;
            let gross = idle + external_value as u128;
            fuzz_assert_eq!(
                net_plus_liabilities,
                gross,
                "NAV report imbalance: total_assets {} + fees {} + pending {} != idle {} + external {}",
                vault.total_assets,
                vault.fees_payable,
                vault.pending_withdrawal_assets,
                idle,
                external_value
            );
        }
        ok
    }

    /// Flip pause flags.
    pub fn action_set_pause_flags(
        &mut self,
        deposits: bool,
        withdrawals: bool,
        manage: bool,
    ) -> bool {
        let ix = roshi_client::instruction::set_pause_flags(
            self.operator.pubkey(),
            self.vault,
            deposits,
            withdrawals,
            manage,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Advance the clock so time-dependent paths (fees, reporting, oracle
    /// staleness) are reachable. LiteSVM's `warp_to_slot` moves `Clock.slot`
    /// only; `unix_timestamp` starts at 0 and never advances on its own, which
    /// would leave the oracle staleness check (`publish_time + max_age < now`)
    /// permanently unreachable. Advance wall time alongside slots — sysvars are
    /// restored every fuzz iteration, so this never leaks across sequences.
    pub fn action_advance_slots(&mut self, #[range(0..32)] slots: u64) -> bool {
        let advanced = slots + 1;
        let target = self.ctx.slot() + advanced;
        self.ctx.warp_to_slot(target);
        let mut clock: Clock = self.ctx.svm.get_sysvar();
        clock.unix_timestamp += advanced as i64 * SECONDS_PER_SLOT;
        self.ctx.set_sysvar(&clock);
        true
    }
}

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
        compute_action_hash_from_metas(&token_program, &ops, &metas, &[SPL_TRANSFER_TAG])
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
