    /// Relay a new base deposit into tracked venue custody. The global NAV,
    /// share, and conservation invariants run after this action as usual.
    pub fn action_deposit_and_deploy(&mut self, #[range(0..NUM_USERS)] user: usize, amount: u64) -> bool {
        // The signer/proof outlive the mutable transaction submission.
        let user = self.users[user].clone();
        let balance = token_balance(&self.ctx.svm, &user.base_ata);
        if balance == 0 { return false; }
        let amount = amount % (balance + 1);
        let before = self.load_vault();
        let custody_before = token_balance(&self.ctx.svm, &self.custody);
        let venue_before = token_balance(&self.ctx.svm, &self.atomic_venue);
        let supply_before = mint_supply(&self.ctx.svm, &self.share_mint);
        let shares_before = token_balance(&self.ctx.svm, &user.share_ata);
        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&amount.to_le_bytes());
        let ix = roshi_client::instruction::deposit_and_deploy(
            user.kp.pubkey(), self.vault, user.base_ata, self.custody, user.share_ata,
            self.share_mint, support::TOKEN_PROGRAM_ID, self.sub_account, self.deploy_action,
            vec![
                AccountMeta::new(self.custody, false),
                AccountMeta::new(self.atomic_venue, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            ],
            DepositAndDeployArgs {
                amount, min_shares_out: 0, access_proof: user.access_proof.clone(),
                accounts_start: 0,
                account_flags: PackedAccountFlags::from_flags(&[
                    AccountFlags { is_signer: false, is_writable: true },
                    AccountFlags { is_signer: false, is_writable: true },
                    AccountFlags { is_signer: false, is_writable: false },
                ]),
                ix_data,
            },
        ).unwrap();
        let ok = submit(&mut self.ctx, ix, &[&user.kp]);
        let after = self.load_vault();
        let supply_after = mint_supply(&self.ctx.svm, &self.share_mint);
        let shares_after = token_balance(&self.ctx.svm, &user.share_ata);
        if ok {
            let expected_shares = shares_for_deposit(
                amount, before.effective_total_assets(self.unix_timestamp()).unwrap(),
                before.economic_share_supply(supply_before).unwrap(), BASE_DECIMALS,
            ).unwrap();
            fuzz_assert_eq!(after.total_assets, before.total_assets + amount, "relay changed NAV");
            fuzz_assert_eq!(supply_after, supply_before + expected_shares, "wrong share supply");
            fuzz_assert_eq!(shares_after, shares_before + expected_shares, "wrong deposited shares");
            fuzz_assert_eq!(token_balance(&self.ctx.svm, &self.atomic_venue), venue_before + amount, "wrong deployed amount");
            fuzz_assert_eq!(token_balance(&self.ctx.svm, &user.base_ata), balance - amount, "wrong deposit debit");
        } else {
            fuzz_assert_eq!(after.total_assets, before.total_assets, "failed relay changed NAV");
            fuzz_assert_eq!(supply_after, supply_before, "failed relay minted shares");
            fuzz_assert_eq!(shares_after, shares_before, "failed relay credited shares");
            fuzz_assert_eq!(token_balance(&self.ctx.svm, &self.atomic_venue), venue_before, "failed relay moved capital");
            fuzz_assert_eq!(token_balance(&self.ctx.svm, &user.base_ata), balance, "failed relay debited depositor");
        }
        fuzz_assert_eq!(token_balance(&self.ctx.svm, &self.custody), custody_before, "relay moved idle base");
        ok
    }
