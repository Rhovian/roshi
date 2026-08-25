fn pyth_config() -> OracleConfig {
    OracleConfig::pyth(PythOracleConfig::new(
        PYTH_FEED_ID,
        PYTH_PRICE_DECIMALS,
        PYTH_MAX_AGE_SECS,
        PYTH_MAX_CONF_BPS,
    ))
}

fn scope_config(&self, price_index: u16) -> OracleConfig {
    OracleConfig::scope(ScopeOracleConfig::new(
        self.scope_prices_account.to_bytes(),
        SCOPE_PRICE_INFO_ACCOUNT,
        SCOPE_PRICE_TYPE,
        price_index,
        SCOPE_MAX_AGE_SECS,
    ))
}

fn update_primary_asset_oracle(&mut self, oracle: OracleConfig) -> bool {
    let ix = roshi_client::instruction::update_asset(
        self.operator.pubkey(),
        self.vault,
        self.asset_pda,
        UpdateAssetArgs {
            oracle,
            enabled: true,
            routed: false,
            deposit_cap_atoms: u64::MAX,
        },
    )
    .unwrap();
    submit(&mut self.ctx, ix, &[&self.operator.clone()])
}

fn restore_primary_asset_pyth(&mut self) -> bool {
    let restored = self.update_primary_asset_oracle(Self::pyth_config());
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
        SCOPE_PRICE_INDEX,
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
) -> solana_instruction::Instruction {
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
            AccountMeta::new_readonly(self.scope_prices_account, false),
            AccountMeta::new_readonly(self.scope_mappings_account, false),
        ],
    )
    .unwrap()
}

fn run_scope_rejection(&mut self, user: usize, amount: u64, reason: &str) -> bool {
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
    let source_before = token_balance(&self.ctx.svm, &user.asset_ata);
    let custody_before = token_balance(&self.ctx.svm, &self.asset_custody);
    let ix = self.deposit_scope_asset_ix(&user, amount);
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
