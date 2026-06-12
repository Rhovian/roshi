    /// Sweep accrued performance fees to the treasury.
    pub fn action_collect_fees(&mut self, amount: u64) -> bool {
        let before = self.load_vault();
        let custody_before = token_balance(&self.ctx.svm, &self.custody);
        let treasury_before = token_balance(&self.ctx.svm, &self.treasury);
        if before.fees_payable == 0 {
            let ix = roshi_client::instruction::collect_fees(
                self.operator.pubkey(),
                self.vault,
                0,
                self.sub_account,
                self.custody,
                self.treasury,
                1,
            )
            .unwrap();
            let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
            let after = self.load_vault();
            let custody_after = token_balance(&self.ctx.svm, &self.custody);
            let treasury_after = token_balance(&self.ctx.svm, &self.treasury);
            fuzz_assert!(
                !ok && after == before && custody_after == custody_before && treasury_after == treasury_before,
                "collect_fees without payable succeeded or mutated state"
            );
            return ok;
        }

        let overpay = before.fees_payable.saturating_add(1);
        let overpay_ix = roshi_client::instruction::collect_fees(
            self.operator.pubkey(),
            self.vault,
            0,
            self.sub_account,
            self.custody,
            self.treasury,
            overpay,
        )
        .unwrap();
        let overpay_ok = submit(&mut self.ctx, overpay_ix, &[&self.operator.clone()]);
        let after_overpay = self.load_vault();
        let custody_after_overpay = token_balance(&self.ctx.svm, &self.custody);
        let treasury_after_overpay = token_balance(&self.ctx.svm, &self.treasury);
        fuzz_assert!(
            !overpay_ok
                && after_overpay == before
                && custody_after_overpay == custody_before
                && treasury_after_overpay == treasury_before,
            "overpay collect_fees succeeded or mutated state"
        );

        let collectable = before.fees_payable.min(custody_before);
        if collectable == 0 {
            return false;
        }
        let amount = (amount % collectable) + 1;
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
        let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        let after = self.load_vault();
        let custody_after = token_balance(&self.ctx.svm, &self.custody);
        let treasury_after = token_balance(&self.ctx.svm, &self.treasury);
        fuzz_assert!(
            ok && after.fees_payable == before.fees_payable - amount
                && after.total_assets == before.total_assets
                && custody_after == custody_before - amount
                && treasury_after == treasury_before + amount,
            "collect_fees accepted wrong accounting: ok={ok}, fees {}->{}, total_assets {}->{}, custody {}->{}, treasury {}->{}, amount={amount}",
            before.fees_payable,
            after.fees_payable,
            before.total_assets,
            after.total_assets,
            custody_before,
            custody_after,
            treasury_before,
            treasury_after
        );
        ok
    }

    /// Report NAV (advances the report epoch — which prices queued withdrawals —
    /// and accrues performance fees). The hash is always unique so the report
    /// isn't rejected as a replay; `external_value` is bounded to the system's
    /// base so NAV math stays in range and the report actually lands.
    pub fn action_report_nav(&mut self, #[range(0..4_000_000_000)] external_value: u64) -> bool {
        let before = self.load_vault();
        let share_supply = mint_supply(&self.ctx.svm, &self.share_mint);
        let economic_share_supply = match before.economic_share_supply(share_supply) {
            Ok(supply) => supply,
            Err(err) => {
                fuzz_assert!(
                    false,
                    "economic share supply overflow before NAV report: {err:?}"
                );
                return false;
            }
        };
        let deposit_idle = token_balance(&self.ctx.svm, &self.custody);
        let withdraw_idle = token_balance(&self.ctx.svm, &self.withdraw_custody);
        let idle = deposit_idle as u128 + withdraw_idle as u128;
        let gross = idle + external_value as u128;
        let expected_fee_base = gross
            .checked_sub(before.fees_payable as u128)
            .and_then(|assets| assets.checked_sub(before.pending_withdrawal_assets as u128))
            .and_then(|assets| u64::try_from(assets).ok());
        let expected_nav = expected_fee_base.and_then(|fee_base| {
            performance_fee_for_nav(
                fee_base,
                economic_share_supply,
                before.high_watermark,
                before.performance_fee_bps,
            )
            .ok()
        });

        // C-controls gating, predicted exactly: the rate limit from the
        // stored timestamp, and the gain bound on the expected net NAV.
        let now = self.unix_timestamp();
        let interval_ok = before.verify_report_interval(now).is_ok();
        let bound_ok = match &expected_nav {
            Some((_, net_total_assets, _)) => before
                .verify_nav_gain_bound(*net_total_assets, economic_share_supply)
                .is_ok(),
            None => true,
        };

        self.report_nonce += 1;
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&self.report_nonce.to_le_bytes());
        let ix = roshi_client::instruction::report_nav(
            self.nav_authority.pubkey(),
            self.vault,
            self.share_mint,
            self.base_mint,
            self.custody,
            self.withdraw_custody,
            external_value,
            hash,
        )
        .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&self.nav_authority.clone()]);
        fuzz_assert!(
            ok == (expected_nav.is_some() && interval_ok && bound_ok),
            "report gating mismatch: ok={ok}, fee_math_ok={}, interval_ok={interval_ok}, bound_ok={bound_ok}",
            expected_nav.is_some()
        );
        if ok {
            // NAV-report conservation: the program's own fee/liability arithmetic
            // must balance the moment a report lands. Gross NAV is idle custody +
            // the reported external value; out of it the program carves accrued
            // fees and pending withdrawals, leaving net `total_assets`. So
            //   total_assets + fees_payable + pending_withdrawal_assets
            //     == idle + external_value.
            // `report_nav` moves no tokens, so idle is unchanged from what the
            // program read. This pins the highest-risk subtraction in the
            // program; a stray over/under-charge of fees or liabilities breaks it
            // even when base conservation still holds. Split custody here means
            // idle is deposit custody plus withdraw custody.
            let vault = self.load_vault();
            let net_plus_liabilities = vault.total_assets as u128
                + vault.fees_payable as u128
                + vault.pending_withdrawal_assets as u128;
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
            if let Some((fee_assets, net_total_assets, high_watermark)) = expected_nav {
                let expected_fees_payable = before.fees_payable + fee_assets;
                fuzz_assert!(
                    vault.fees_payable == expected_fees_payable
                        && vault.total_assets == net_total_assets
                        && vault.high_watermark == high_watermark,
                    "NAV fee accounting mismatch: expected fee +{} => fees {}, total_assets {}, hwm {}; got fees {}, total_assets {}, hwm {}",
                    fee_assets,
                    expected_fees_payable,
                    net_total_assets,
                    high_watermark,
                    vault.fees_payable,
                    vault.total_assets,
                    vault.high_watermark
                );
            } else {
                fuzz_assert!(
                    false,
                    "report_nav succeeded when expected fee math rejected: gross={}, fees_payable={}, pending={}, economic_supply={}, hwm={}, fee_bps={}",
                    gross,
                    before.fees_payable,
                    before.pending_withdrawal_assets,
                    economic_share_supply,
                    before.high_watermark,
                    before.performance_fee_bps
                );
            }
        }
        ok
    }
