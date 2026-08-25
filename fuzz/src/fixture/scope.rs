fn pyth_config() -> OracleConfig {
    OracleConfig::pyth(PythOracleConfig::new(
        PYTH_FEED_ID,
        PYTH_PRICE_DECIMALS,
        PYTH_MAX_AGE_SECS,
        PYTH_MAX_CONF_BPS,
    ))
}

fn scope_config(&self, price_index: u16) -> OracleConfig {
    self.scope_config_with_max_age(price_index, SCOPE_MAX_AGE_SECS)
}

fn scope_config_with_max_age(&self, price_index: u16, max_age_seconds: u64) -> OracleConfig {
    OracleConfig::scope(ScopeOracleConfig::new(
        self.scope_prices_account.to_bytes(),
        SCOPE_PRICE_INFO_ACCOUNT,
        SCOPE_PRICE_TYPE,
        price_index,
        max_age_seconds,
    ))
}

fn update_vault_pricing(&mut self, base_oracle: OracleConfig, controls: VaultControls) -> bool {
    let vault = self.load_vault();
    let ix = roshi_client::instruction::update_vault_config(
        self.operator.pubkey(),
        self.vault,
        UpdateVaultConfigArgs {
            treasury: vault.treasury,
            deposit_sub_account: vault.deposit_sub_account,
            withdraw_sub_account: vault.withdraw_sub_account,
            base_oracle,
            performance_fee_bps: vault.performance_fee_bps,
            withdrawal_buffer_bps: vault.withdrawal_buffer_bps,
            deposit_cap: vault.deposit_cap,
            controls,
            external_enabled: vault
                .external_enabled()
                .expect("loaded vault has valid flags"),
        },
    )
    .unwrap();
    submit(&mut self.ctx, ix, &[&self.operator.clone()])
}

fn update_primary_asset_oracle(&mut self, oracle: OracleConfig, routed: bool) -> bool {
    let ix = roshi_client::instruction::update_asset(
        self.operator.pubkey(),
        self.vault,
        self.asset_pda,
        UpdateAssetArgs {
            oracle,
            enabled: true,
            routed,
            deposit_cap_atoms: u64::MAX,
        },
    )
    .unwrap();
    submit(&mut self.ctx, ix, &[&self.operator.clone()])
}

fn restore_primary_asset_pyth(&mut self) -> bool {
    let restored = self.update_primary_asset_oracle(Self::pyth_config(), false);
    fuzz_assert!(restored, "failed to restore the primary asset's Pyth config");
    restored
}

fn scope_now(&mut self) -> u64 {
    let mut clock: Clock = self.ctx.svm.get_sysvar();
    if clock.unix_timestamp <= SCOPE_MAX_AGE_SECS as i64 {
        clock.unix_timestamp = SCOPE_MAX_AGE_SECS as i64 + 1;
        self.ctx.set_sysvar(&clock);
    }
    u64::try_from(clock.unix_timestamp).expect("fuzz clock is nonnegative")
}

fn write_scope_account(
    &mut self,
    address: Pubkey,
    data: Vec<u8>,
    owner: Pubkey,
    label: &str,
) {
    let lamports = self
        .ctx
        .get_account(&address)
        .unwrap_or_else(|_| panic!("{label} account missing"))
        .lamports;
    self.ctx
        .write_account(
            &address,
            Account {
                lamports,
                data,
                owner,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap_or_else(|_| panic!("write {label} account"));
}

fn write_scope_pair(
    &mut self,
    declared_mappings: Pubkey,
    price_index: u16,
    value: u64,
    exponent: u64,
    timestamp: u64,
    price_type: u8,
    mapped_price_info_account: [u8; 32],
    prices_owner: Pubkey,
    mappings_owner: Pubkey,
    malformed_prices: bool,
    malformed_mappings: bool,
) {
    let (mut prices, mut mappings) = scope_oracle_data(
        declared_mappings,
        price_index,
        value,
        exponent,
        timestamp,
        price_type,
        mapped_price_info_account,
    );
    if malformed_prices {
        prices[0] ^= 0xff;
    }
    if malformed_mappings {
        mappings.truncate(mappings.len() - 1);
    }
    self.write_scope_account(
        self.scope_prices_account,
        prices,
        prices_owner,
        "Scope prices",
    );
    self.write_scope_account(
        self.scope_mappings_account,
        mappings,
        mappings_owner,
        "Scope mappings",
    );
}

fn deposit_scope_asset_ix(
    &self,
    user: &FuzzUser,
    amount: u64,
    routed: bool,
) -> solana_instruction::Instruction {
    let mut oracle_accounts = vec![
        AccountMeta::new_readonly(self.asset_pda, false),
        AccountMeta::new_readonly(self.scope_prices_account, false),
        AccountMeta::new_readonly(self.scope_mappings_account, false),
    ];
    if routed {
        oracle_accounts.push(AccountMeta::new_readonly(self.pyth_account, false));
    }
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
        oracle_accounts,
    )
    .unwrap()
}

fn run_scope_rejection(&mut self, user: usize, amount: u64, reason: &str) -> bool {
    let config = self.scope_config(SCOPE_PRICE_INDEX);
    self.run_scope_rejection_with_config(user, amount, reason, config)
}

fn run_scope_rejection_with_config(
    &mut self,
    user: usize,
    amount: u64,
    reason: &str,
    config: OracleConfig,
) -> bool {
    fuzz_assert!(
        self.update_primary_asset_oracle(config, false),
        "valid Scope config rejected"
    );
    let user = self.users[user].clone();
    let balance = token_balance(&self.ctx.svm, &user.asset_ata);
    if balance == 0 {
        self.restore_primary_asset_pyth();
        return false;
    }
    let amount = (amount % balance) + 1;
    let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
    let ix = self.deposit_scope_asset_ix(&user, amount, false);
    let ok = submit(&mut self.ctx, ix, &[&user.kp]);
    let source_after = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_after = token_balance(&self.ctx.svm, &self.asset_custody);
    fuzz_assert!(
        !ok && source_after == source_before && custody_after == custody_before,
        "Scope deposit admitted despite {reason}: ok={ok}, source {source_before}->{source_after}, custody {custody_before}->{custody_after}"
    );
    self.restore_primary_asset_pyth();
    ok
}
