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

    /// Transfer-fee Token-2022 mints must be rejected by `initialize_asset` before
    /// the Asset PDA is created. Bare 82-byte Token-2022 mints are covered by
    /// setup and `action_deposit_token_2022_asset`; this pins the opposite edge.
    pub fn action_initialize_transfer_fee_token_2022_asset_rejects(&mut self) -> bool {
        let ix = roshi_client::instruction::initialize_asset(
            self.operator.pubkey(),
            self.vault,
            self.transfer_fee_token_2022_mint,
            self.transfer_fee_token_2022_asset_pda,
            InitializeAssetArgs {
                asset_mint: self.transfer_fee_token_2022_mint.to_bytes(),
                oracle: OracleConfig::pyth(PythOracleConfig::new(
                    PYTH_FEED_ID,
                    PYTH_PRICE_DECIMALS,
                    PYTH_MAX_AGE_SECS,
                    PYTH_MAX_CONF_BPS,
                )),
                asset_decimals: ASSET_DECIMALS,
                enabled: true,
                routed: false,
                deposit_cap_atoms: u64::MAX,
            },
        )
        .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        let created = self
            .ctx
            .get_account(&self.transfer_fee_token_2022_asset_pda)
            .is_ok();
        fuzz_assert!(
            !ok && !created,
            "transfer-fee Token-2022 mint initialized as asset: success={ok}, created={created}"
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
                routed: false,
                deposit_cap_atoms: u64::MAX,
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
                routed: false,
                deposit_cap_atoms: u64::MAX,
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
