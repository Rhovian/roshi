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
