/// Decode the registered classic asset's full state.
fn load_asset(&self) -> roshi::state::asset::Asset {
    let account = self.ctx.get_account(&self.asset_pda).expect("asset exists");
    match wincode::deserialize::<RoshiAccount>(&account.data) {
        Ok(RoshiAccount::Asset(asset)) => asset,
        Ok(_) => panic!("asset PDA is not an Asset account"),
        Err(_) => panic!("asset PDA failed to deserialize"),
    }
}

/// Toggle the vault's economic controls between fully off (the corpus
/// baseline) and a fixed exercising profile: profit drip, staleness gate,
/// report rate limit, NAV gain bound, cancel grace, and the atomic exit fee.
/// Swap slippage stays off here — the fuzz swap venues are plain transfers
/// whose oracle-valuation plumbing is pinned by integration tests, and
/// pricing them would force oracle accounts into every swap action.
pub fn action_set_controls(&mut self, enabled: bool) -> bool {
    let before = self.load_vault();
    let controls = if enabled {
        VaultControls::new(600, 3_600, 60, 5_000, 5_000, 100, 0)
    } else {
        VaultControls::default()
    };

    let external_enabled = before.external_enabled().unwrap_or(false);
    let ix = roshi_client::instruction::update_vault_config(
        self.operator.pubkey(),
        self.vault,
        UpdateVaultConfigArgs {
            treasury: before.treasury,
            deposit_sub_account: before.deposit_sub_account,
            withdraw_sub_account: before.withdraw_sub_account,
            base_oracle: before.base_oracle,
            performance_fee_bps: before.performance_fee_bps,
            withdrawal_buffer_bps: before.withdrawal_buffer_bps,
            controls,
            external_enabled,
        },
    )
    .unwrap();
    let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
    let after = self.load_vault();
    fuzz_assert!(
        ok && after.controls == controls,
        "set_controls(enabled={enabled}) rejected or stored wrong config: ok={ok}"
    );
    true
}

/// Advance the clock: time is what drips locked profit, ages reports for the
/// staleness gate, and spaces the report rate limit. Slots move with it so
/// slot-based windows (cancel delay/grace) age too.
pub fn action_advance_time(&mut self, #[range(1..3_600)] secs: i64) -> bool {
    let mut clock: Clock = self.ctx.svm.get_sysvar();
    clock.unix_timestamp = clock.unix_timestamp.saturating_add(secs);
    clock.slot = clock.slot.saturating_add(secs as u64 * 2);
    self.ctx.set_sysvar(&clock);
    true
}

/// Forgive accrued fee liability. Writing down more than `fees_payable` (or
/// zero) must reject without mutation; a valid writedown must shrink only
/// `fees_payable` — never `total_assets`, never a token balance (the global
/// conservation invariant double-checks the latter).
pub fn action_write_down_fees(&mut self, amount: u64) -> bool {
    let before = self.load_vault();

    let overpay = before.fees_payable.saturating_add(1);
    let overpay_ix =
        roshi_client::instruction::write_down_fees(self.operator.pubkey(), self.vault, overpay)
            .unwrap();
    let overpay_ok = submit(&mut self.ctx, overpay_ix, &[&self.operator.clone()]);
    let after_overpay = self.load_vault();
    fuzz_assert!(
        !overpay_ok && after_overpay == before,
        "over-writedown of {overpay} (payable {}) succeeded or mutated state",
        before.fees_payable
    );

    if before.fees_payable == 0 {
        return false;
    }
    let amount = (amount % before.fees_payable) + 1;
    let ix = roshi_client::instruction::write_down_fees(self.operator.pubkey(), self.vault, amount)
        .unwrap();
    let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
    let after = self.load_vault();
    fuzz_assert!(
        ok && after.fees_payable == before.fees_payable - amount
            && after.total_assets == before.total_assets,
        "writedown of {amount} (payable {}) rejected or mis-accounted: ok={ok}, fees {}->{}, total {}->{}",
        before.fees_payable,
        before.fees_payable,
        after.fees_payable,
        before.total_assets,
        after.total_assets
    );
    true
}

/// The per-asset inventory cap binds: with the cap set just below
/// `custody_balance + amount`, that deposit must reject and move nothing,
/// whatever other gates are active. The cap is restored afterwards so the
/// rest of the sequence keeps its deposit surface.
pub fn action_asset_deposit_cap_binds(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
) -> bool {
    let user = self.users[user].clone();
    let balance = token_balance(&self.ctx.svm, &user.asset_ata);
    if balance == 0 {
        return false;
    }
    let amount = (amount % balance) + 1;

    let asset = self.load_asset();
    let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
    let Some(boundary) = custody_before.checked_add(amount) else {
        return false;
    };
    let routed = asset.routed().expect("asset routed flag decodes");
    let enabled = asset.enabled().expect("asset enabled flag decodes");
    let (operator_key, vault_key, asset_pda, oracle) = (
        self.operator.pubkey(),
        self.vault,
        self.asset_pda,
        asset.oracle,
    );
    let set_cap = move |cap: u64| {
        roshi_client::instruction::update_asset(
            operator_key,
            vault_key,
            asset_pda,
            UpdateAssetArgs {
                oracle,
                enabled,
                routed,
                deposit_cap_atoms: cap,
            },
        )
        .unwrap()
    };

    let cap_ok = submit(&mut self.ctx, set_cap(boundary - 1), &[&self.operator.clone()]);
    fuzz_assert!(cap_ok, "admin update_asset(cap) rejected");

    self.write_pyth_price(0, self.unix_timestamp());
    let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
    let ix = self.deposit_asset_ix(&user, amount);
    let ok = submit(&mut self.ctx, ix, &[&user.kp]);
    let custody_after = token_balance(&self.ctx.svm, &self.asset_custody);
    let source_after = token_balance(&self.ctx.svm, &user.asset_ata);
    fuzz_assert!(
        !ok && custody_after == custody_before && source_after == source_before,
        "deposit above the inventory cap landed: amount={amount}, custody={custody_before}, cap={}",
        boundary - 1
    );

    let restore_ok = submit(
        &mut self.ctx,
        set_cap(asset.deposit_cap_atoms),
        &[&self.operator.clone()],
    );
    fuzz_assert!(restore_ok, "admin update_asset(cap restore) rejected");
    true
}
