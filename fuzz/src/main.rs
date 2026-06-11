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

use crucible_fuzzer::*;
use std::rc::Rc;

use roshi::{
    instructions::{
        AccountFlags, AtomicRedeemArgs, InitializeVaultArgs, ManageArgs, SwapArgs,
        UpdateVaultConfigArgs,
    },
    oracle::OracleConfig,
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Op, Ops},
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
    math::assets_for_redeem,
};
use solana_instruction::AccountMeta;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

/// SPL Token `Transfer` instruction discriminator (classic token program).
const SPL_TRANSFER_TAG: u8 = 3;

mod support;
use support::{mint_supply, set_ata, set_mint, set_token_account, token_balance};

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

#[derive(Clone)]
struct FuzzUser {
    kp: Rc<Keypair>,
    base_ata: Pubkey,
    share_ata: Pubkey,
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

        // 5. Users, each funded with base; share ATA starts empty.
        let mut users = Vec::with_capacity(NUM_USERS);
        let mut base_accounts = vec![custody, swap_custody, atomic_venue, external_account, treasury];
        for _ in 0..NUM_USERS {
            let kp = Rc::new(Keypair::new());
            ctx.svm.airdrop(&kp.pubkey(), FUND_LAMPORTS).unwrap();
            let base_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &base_mint, INITIAL_USER_BASE);
            let share_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &share_mint, 0);
            base_accounts.push(base_ata);
            users.push(FuzzUser {
                kp,
                base_ata,
                share_ata,
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
        base_accounts.push(outsider_base);
        let outsider = FuzzUser {
            kp: outsider_kp,
            base_ata: outsider_base,
            share_ata: outsider_share,
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

    /// Advance the clock so time-dependent paths (fees, reporting) are reachable.
    pub fn action_advance_slots(&mut self, #[range(0..32)] slots: u64) -> bool {
        let target = self.ctx.slot() + slots + 1;
        self.ctx.warp_to_slot(target);
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
        compute_action_hash_from_metas(&support::TOKEN_PROGRAM_ID, &ops, &metas, &[SPL_TRANSFER_TAG])
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
