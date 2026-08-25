/// Exercise every supported Scope exponent and fresh-age boundary through a
/// real asset deposit. The encoded value always represents exactly 2.0.
pub fn action_deposit_asset_scope_fresh(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
    #[range(0..19)] exponent: u32,
    #[range(0..SCOPE_MAX_AGE_SECS + 1)] age: u64,
) -> bool {
    let now = self.scope_now();
    let value = 2u64
        .checked_mul(10u64.pow(exponent))
        .expect("2 * 10^18 fits u64");
    self.write_scope_pair(
        self.scope_mappings_account,
        value,
        u64::from(exponent),
        now - age,
        SCOPE_PRICE_TYPE,
        SCOPE_FEED_ID,
        SCOPE_PROGRAM,
        SCOPE_PROGRAM,
        false,
        false,
    );
    fuzz_assert!(
        self.update_primary_asset_oracle(self.scope_config(SCOPE_PRICE_INDEX)),
        "valid Scope config rejected"
    );

    let user = self.users[user].clone();
    let balance = token_balance(&self.ctx.svm, &user.asset_ata);
    if balance == 0 {
        self.restore_primary_asset_pyth();
        return false;
    }
    let amount = (amount % balance) + 1;
    let vault = self.load_vault();
    let should_succeed = !vault.deposits_paused().unwrap_or(true)
        && self.fresh_asset_deposit_can_reach_transfer(&vault, amount);
    let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
    let ix = self.deposit_scope_asset_ix(&user, amount);
    let ok = submit(&mut self.ctx, ix, &[&user.kp]);
    let source_after = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_after = token_balance(&self.ctx.svm, &self.asset_custody);
    if should_succeed {
        fuzz_assert!(
            ok && source_after == source_before - amount
                && custody_after == custody_before + amount,
            "fresh Scope price rejected or moved wrong amount: ok={ok}, exp={exponent}, age={age}, source {source_before}->{source_after}, custody {custody_before}->{custody_after}"
        );
    }
    self.restore_primary_asset_pyth();
    ok
}

/// Mutate the price and mapping entry. At least one rejection condition is
/// always present, including when all generated booleans are false.
pub fn action_deposit_asset_scope_rejects_entry(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
    zero_value: bool,
    bad_exponent: bool,
    wrong_feed: bool,
    wrong_type: bool,
    frozen: bool,
) -> bool {
    let no_flag = !(zero_value || bad_exponent || wrong_feed || wrong_type || frozen);
    let now = self.scope_now();
    self.write_scope_pair(
        self.scope_mappings_account,
        if zero_value || no_flag {
            0
        } else {
            200_000_000_000_000_000
        },
        if bad_exponent { 19 } else { 17 },
        now,
        if wrong_type {
            SCOPE_PRICE_TYPE - 1
        } else if frozen {
            SCOPE_PRICE_TYPE | SCOPE_FROZEN_FLAG
        } else {
            SCOPE_PRICE_TYPE
        },
        if wrong_feed { [9; 32] } else { SCOPE_FEED_ID },
        SCOPE_PROGRAM,
        SCOPE_PROGRAM,
        false,
        false,
    );
    self.run_scope_rejection(user, amount, "invalid price or mapping entry")
}

/// Drive stale and future observation rejection at the one-second boundaries.
pub fn action_deposit_asset_scope_rejects_timestamp(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
    future: bool,
) -> bool {
    let now = self.scope_now();
    let timestamp = if future {
        now + 1
    } else {
        now - SCOPE_MAX_AGE_SECS - 1
    };
    self.write_scope_pair(
        self.scope_mappings_account,
        200_000_000_000_000_000,
        17,
        timestamp,
        SCOPE_PRICE_TYPE,
        SCOPE_FEED_ID,
        SCOPE_PROGRAM,
        SCOPE_PROGRAM,
        false,
        false,
    );
    self.run_scope_rejection(user, amount, "stale or future observation")
}

/// Mutate account-level trust checks: owners, embedded mappings pointer,
/// discriminator, and exact mappings length.
pub fn action_deposit_asset_scope_rejects_accounts(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
    wrong_prices_owner: bool,
    wrong_mappings_owner: bool,
    malformed_prices: bool,
    malformed_mappings: bool,
) -> bool {
    let no_flag = !(wrong_prices_owner
        || wrong_mappings_owner
        || malformed_prices
        || malformed_mappings);
    let now = self.scope_now();
    self.write_scope_pair(
        if no_flag {
            Pubkey::new_unique()
        } else {
            self.scope_mappings_account
        },
        200_000_000_000_000_000,
        17,
        now,
        SCOPE_PRICE_TYPE,
        SCOPE_FEED_ID,
        if wrong_prices_owner {
            Pubkey::new_unique()
        } else {
            SCOPE_PROGRAM
        },
        if wrong_mappings_owner {
            Pubkey::new_unique()
        } else {
            SCOPE_PROGRAM
        },
        malformed_prices,
        malformed_mappings,
    );
    self.run_scope_rejection(user, amount, "invalid Scope account contract")
}

/// The config boundary is enforced before state changes: index 511 is legal,
/// while 512 must reject and leave the Asset byte-for-byte unchanged.
pub fn action_scope_index_bound(&mut self) -> bool {
    let before = self.load_asset();
    let last_ok = self.update_primary_asset_oracle(self.scope_config(511));
    fuzz_assert!(last_ok, "last in-range Scope index rejected");
    let valid = self.load_asset();
    let invalid_ok = self.update_primary_asset_oracle(self.scope_config(512));
    let after = self.load_asset();
    fuzz_assert!(
        !invalid_ok && after == valid,
        "out-of-range Scope index succeeded or mutated the Asset"
    );
    let restored = self.restore_primary_asset_pyth();
    fuzz_assert!(
        before.enabled() == after.enabled(),
        "Scope config update changed flags"
    );
    last_ok && restored
}
