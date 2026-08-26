/// Exercise Scope exponents through 19 and the fresh-age boundary through a
/// real asset deposit. The encoded value represents 2.0 through exponent 18
/// and 1.0 at exponent 19, the largest power of ten that fits a `u64`.
pub fn action_deposit_asset_scope_fresh(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
    #[range(0..20)] exponent: u32,
    #[range(0..SCOPE_MAX_AGE_SECS + 1)] age: u64,
    last_index: bool,
) -> bool {
    let now = self.scope_now();
    let price_index = if last_index {
        ScopeOracleConfig::MAX_ENTRIES - 1
    } else {
        SCOPE_PRICE_INDEX
    };
    let scale = 10u64.pow(exponent);
    let value = if exponent == 19 { scale } else { 2 * scale };
    self.write_scope_pair(
        self.scope_mappings_account,
        price_index,
        value,
        u64::from(exponent),
        now - age,
        SCOPE_MAPPING,
        false,
        SCOPE_PROGRAM_ID,
        SCOPE_PROGRAM_ID,
        false,
        false,
    );
    fuzz_assert!(
        self.update_primary_asset_oracle(self.scope_config(price_index), false),
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
    let should_succeed = !vault
        .deposits_paused()
        .expect("loaded vault has valid flags")
        && self.fresh_asset_deposit_can_reach_transfer(&vault, amount);
    let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
    let ix = self.deposit_scope_asset_ix(&user, amount, false);
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

/// Route a Scope-priced asset through the vault's Pyth base leg. This pins the
/// account boundary after Scope's two accounts under arbitrary deposit state.
pub fn action_deposit_asset_scope_routed(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
) -> bool {
    let original_vault = self.load_vault();
    fuzz_assert!(
        self.update_vault_pricing(Self::pyth_config(), original_vault.controls),
        "failed to install the routed deposit's Pyth base oracle"
    );
    let now = self.scope_now();
    self.write_scope_pair(
        self.scope_mappings_account,
        SCOPE_PRICE_INDEX,
        200_000_000_000_000_000,
        17,
        now,
        SCOPE_MAPPING,
        false,
        SCOPE_PROGRAM_ID,
        SCOPE_PROGRAM_ID,
        false,
        false,
    );
    self.write_pyth_price(
        0,
        i64::try_from(now).expect("Scope time originated from the i64 clock"),
    );
    fuzz_assert!(
        self.update_primary_asset_oracle(self.scope_config(SCOPE_PRICE_INDEX), true),
        "valid routed Scope config rejected"
    );

    let user = self.users[user].clone();
    let balance = token_balance(&self.ctx.svm, &user.asset_ata);
    if balance == 0 {
        self.restore_primary_asset_pyth();
        fuzz_assert!(
            self.update_vault_pricing(original_vault.base_oracle, original_vault.controls),
            "failed to restore the vault after a routed Scope deposit"
        );
        return false;
    }
    let amount = (amount % balance) + 1;
    let vault = self.load_vault();
    let should_succeed = !vault
        .deposits_paused()
        .expect("loaded vault has valid flags")
        && self.fresh_asset_deposit_can_reach_transfer(&vault, amount);
    let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
    let ix = self.deposit_scope_asset_ix(&user, amount, true);
    let ok = submit(&mut self.ctx, ix, &[&user.kp]);
    let source_after = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_after = token_balance(&self.ctx.svm, &self.asset_custody);
    if should_succeed {
        fuzz_assert!(
            ok && source_after == source_before - amount
                && custody_after == custody_before + amount,
            "routed Scope deposit rejected or moved wrong amount: ok={ok}, source {source_before}->{source_after}, custody {custody_before}->{custody_after}"
        );
    }
    self.restore_primary_asset_pyth();
    fuzz_assert!(
        self.update_vault_pricing(original_vault.base_oracle, original_vault.controls),
        "failed to restore the vault after a routed Scope deposit"
    );
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
    wrong_price_info_account: bool,
    wrong_type: bool,
    wrong_twap_source_or_tolerance: bool,
    wrong_twap_enabled_bitmask: bool,
    wrong_ref_price: bool,
    wrong_generic: bool,
    frozen: bool,
) -> bool {
    let no_flag = !(zero_value
        || bad_exponent
        || wrong_price_info_account
        || wrong_type
        || wrong_twap_source_or_tolerance
        || wrong_twap_enabled_bitmask
        || wrong_ref_price
        || wrong_generic
        || frozen);
    let now = self.scope_now();
    let mapping = ScopeOracleMapping::new(
        if wrong_price_info_account {
            [9; 32]
        } else {
            SCOPE_PRICE_INFO_ACCOUNT
        },
        if wrong_type {
            SCOPE_PRICE_TYPE - 1
        } else {
            SCOPE_PRICE_TYPE
        },
        if wrong_twap_source_or_tolerance {
            SCOPE_TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS + 1
        } else {
            SCOPE_TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS
        },
        if wrong_twap_enabled_bitmask {
            SCOPE_TWAP_ENABLED_BITMASK + 1
        } else {
            SCOPE_TWAP_ENABLED_BITMASK
        },
        if wrong_ref_price {
            SCOPE_REF_PRICE + 1
        } else {
            SCOPE_REF_PRICE
        },
        if wrong_generic {
            [11; 20]
        } else {
            SCOPE_GENERIC
        },
    );
    self.write_scope_pair(
        self.scope_mappings_account,
        SCOPE_PRICE_INDEX,
        if zero_value || no_flag {
            0
        } else {
            200_000_000_000_000_000
        },
        if bad_exponent { 256 } else { 17 },
        now,
        mapping,
        frozen,
        SCOPE_PROGRAM_ID,
        SCOPE_PROGRAM_ID,
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
        SCOPE_PRICE_INDEX,
        200_000_000_000_000_000,
        17,
        timestamp,
        SCOPE_MAPPING,
        false,
        SCOPE_PROGRAM_ID,
        SCOPE_PROGRAM_ID,
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
    wrong_mappings_discriminator: bool,
    oversized_prices: bool,
    oversized_mappings: bool,
) -> bool {
    let no_flag = !(wrong_prices_owner
        || wrong_mappings_owner
        || malformed_prices
        || malformed_mappings
        || wrong_mappings_discriminator
        || oversized_prices
        || oversized_mappings);
    let truncate_mappings = malformed_mappings && !oversized_mappings;
    let now = self.scope_now();
    self.write_scope_pair(
        if no_flag {
            Pubkey::new_unique()
        } else {
            self.scope_mappings_account
        },
        SCOPE_PRICE_INDEX,
        200_000_000_000_000_000,
        17,
        now,
        SCOPE_MAPPING,
        false,
        if wrong_prices_owner {
            Pubkey::new_unique()
        } else {
            SCOPE_PROGRAM_ID
        },
        if wrong_mappings_owner {
            Pubkey::new_unique()
        } else {
            SCOPE_PROGRAM_ID
        },
        malformed_prices,
        truncate_mappings,
    );
    if oversized_prices {
        let account = self
            .ctx
            .get_account(&self.scope_prices_account)
            .expect("Scope prices account exists");
        let mut data = account.data;
        data.push(0);
        self.write_scope_account(
            self.scope_prices_account,
            data,
            account.owner,
            "Scope prices",
        );
    }
    if wrong_mappings_discriminator || oversized_mappings {
        let account = self
            .ctx
            .get_account(&self.scope_mappings_account)
            .expect("Scope mappings account exists");
        let mut data = account.data;
        if wrong_mappings_discriminator {
            data[0] ^= 0xff;
        }
        if oversized_mappings {
            data.push(0);
        }
        self.write_scope_account(
            self.scope_mappings_account,
            data,
            account.owner,
            "Scope mappings",
        );
    }
    self.run_scope_rejection(user, amount, "invalid Scope account contract")
}

/// Reject a config that pins a different prices account and reject Scope reads
/// when the cluster clock is negative. Both paths must leave tokens unmoved.
pub fn action_deposit_asset_scope_rejects_config_or_clock(
    &mut self,
    #[range(0..NUM_USERS)] user: usize,
    amount: u64,
    wrong_prices_account: bool,
    negative_clock: bool,
) -> bool {
    let no_flag = !(wrong_prices_account || negative_clock);
    let now = self.scope_now();
    self.write_scope_pair(
        self.scope_mappings_account,
        SCOPE_PRICE_INDEX,
        200_000_000_000_000_000,
        17,
        now,
        SCOPE_MAPPING,
        false,
        SCOPE_PROGRAM_ID,
        SCOPE_PROGRAM_ID,
        false,
        false,
    );

    let config = OracleConfig::scope(ScopeOracleConfig::new(
        if wrong_prices_account || no_flag {
            Pubkey::new_unique().to_bytes()
        } else {
            self.scope_prices_account.to_bytes()
        },
        SCOPE_MAPPING,
        SCOPE_PRICE_INDEX,
        SCOPE_MAX_AGE_SECS,
    ));
    let original_clock: Clock = self.ctx.svm.get_sysvar();
    if negative_clock {
        let mut negative = original_clock.clone();
        negative.unix_timestamp = -1;
        self.ctx.set_sysvar(&negative);
    }
    let ok = self.run_scope_rejection_with_config(
        user,
        amount,
        "wrong prices pin or negative cluster time",
        config,
    );
    if negative_clock {
        self.ctx.set_sysvar(&original_clock);
    }
    ok
}

/// The config boundary is enforced before state changes: the last in-range
/// index is legal, while `MAX_ENTRIES` must reject and leave the Asset
/// byte-for-byte unchanged.
pub fn action_scope_index_bound(&mut self) -> bool {
    let before = self.load_asset();
    let last_index = ScopeOracleConfig::MAX_ENTRIES - 1;
    let last_ok = self.update_primary_asset_oracle(self.scope_config(last_index), false);
    fuzz_assert!(last_ok, "last in-range Scope index rejected");
    let valid = self.load_asset();
    let invalid_ok =
        self.update_primary_asset_oracle(self.scope_config(ScopeOracleConfig::MAX_ENTRIES), false);
    let after = self.load_asset();
    fuzz_assert!(
        !invalid_ok && after == valid,
        "out-of-range Scope index succeeded or mutated the Asset"
    );
    let frozen_type = OracleConfig::scope(ScopeOracleConfig::new(
        self.scope_prices_account.to_bytes(),
        ScopeOracleMapping::new(
            SCOPE_PRICE_INFO_ACCOUNT,
            FROZEN_FLAG,
            SCOPE_TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS,
            SCOPE_TWAP_ENABLED_BITMASK,
            SCOPE_REF_PRICE,
            SCOPE_GENERIC,
        ),
        last_index,
        SCOPE_MAX_AGE_SECS,
    ));
    let invalid_type_ok = self.update_primary_asset_oracle(frozen_type, false);
    let after_type = self.load_asset();
    fuzz_assert!(
        !invalid_type_ok && after_type == valid,
        "Scope price type with the frozen bit succeeded or mutated the Asset"
    );
    let restored = self.restore_primary_asset_pyth();
    fuzz_assert!(
        before.enabled() == after.enabled(),
        "Scope config update changed flags"
    );
    last_ok && restored
}
