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

    /// Flat-NAV round-trip sufficiency: a user who deposits base and immediately
    /// atomically redeems exactly the shares minted by that deposit must not
    /// finish with more base than they started with. This uses the already
    /// authorized unwind CPI, so the probe has no async withdrawal-ticket side
    /// effects and stays isolated from unrelated NAV moves.
    pub fn action_deposit_redeem_flat_nav_no_overpay(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        let vault = self.load_vault();
        if vault.deposits_paused().unwrap_or(true) || vault.withdrawals_paused().unwrap_or(true) {
            return false;
        }

        let user = self.users[user].clone();
        let base_before = token_balance(&self.ctx.svm, &user.base_ata);
        let shares_before = token_balance(&self.ctx.svm, &user.share_ata);
        if base_before == 0 {
            return false;
        }
        let amount = (amount % base_before) + 1;

        let deposit_ix = roshi_client::instruction::deposit(
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
        let deposit_ok = submit(&mut self.ctx, deposit_ix, &[&user.kp]);
        if !deposit_ok {
            return false;
        }

        let shares_after_deposit = token_balance(&self.ctx.svm, &user.share_ata);
        let Some(minted_shares) = shares_after_deposit.checked_sub(shares_before) else {
            fuzz_assert!(false, "deposit reduced the depositor's shares");
            return true;
        };
        if minted_shares == 0 {
            fuzz_assert!(false, "successful deposit minted zero shares");
            return true;
        }

        let after_deposit = self.load_vault();
        let share_supply = mint_supply(&self.ctx.svm, &self.share_mint);
        let Ok(economic_share_supply) = after_deposit.economic_share_supply(share_supply) else {
            fuzz_assert!(false, "economic share supply overflow after flat NAV deposit");
            return true;
        };
        let Ok(max_owed) = assets_for_redeem(
            minted_shares,
            after_deposit.total_assets,
            economic_share_supply,
            BASE_DECIMALS,
        ) else {
            return true;
        };
        let unwind = max_owed.min(token_balance(&self.ctx.svm, &self.atomic_venue));
        if unwind == 0 {
            return true;
        }

        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&unwind.to_le_bytes());
        let atomic_ix = roshi_client::instruction::atomic_redeem(
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
                shares: minted_shares,
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
        let atomic_ok = submit(&mut self.ctx, atomic_ix, &[&user.kp]);
        if !atomic_ok {
            return true;
        }

        let base_after = token_balance(&self.ctx.svm, &user.base_ata);
        let shares_after = token_balance(&self.ctx.svm, &user.share_ata);
        fuzz_assert!(
            shares_after == shares_before && base_after <= base_before && unwind <= amount,
            "flat-NAV deposit/atomic-redeem overpaid: \
             base {base_before}->{base_after}, shares {shares_before}->{shares_after}, \
             deposited={amount}, unwind={unwind}, max_owed={max_owed}"
        );
        true
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
            self.withdrawal_authority.pubkey(),
            self.vault,
            self.withdraw_sub_account,
            self.withdraw_custody,
            self.share_mint,
            settlements,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.withdrawal_authority.clone()])
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
            for ti in 0..=u8::MAX {
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
