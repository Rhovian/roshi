    /// Transfer the program-level authority to an alternate signer, prove the
    /// old authority can no longer transfer it, then restore the original
    /// authority so setup-like paths remain reachable.
    pub fn action_transfer_program_authority(&mut self) -> bool {
        let transfer = roshi_client::instruction::transfer_program_authority(
            self.operator.pubkey(),
            self.config_pda,
            self.program_authority_alt.pubkey(),
        )
        .unwrap();
        if !submit(&mut self.ctx, transfer, &[&self.operator.clone()]) {
            return false;
        }

        let stale = roshi_client::instruction::transfer_program_authority(
            self.operator.pubkey(),
            self.config_pda,
            self.operator.pubkey(),
        )
        .unwrap();
        let stale_ok = submit(&mut self.ctx, stale, &[&self.operator.clone()]);
        fuzz_assert!(
            !stale_ok,
            "old program authority transferred authority after rotation"
        );

        let restore = roshi_client::instruction::transfer_program_authority(
            self.program_authority_alt.pubkey(),
            self.config_pda,
            self.operator.pubkey(),
        )
        .unwrap();
        submit(&mut self.ctx, restore, &[&self.program_authority_alt.clone()])
    }

    /// Transfer vault admin to an alternate signer, prove the old admin no
    /// longer controls the vault, then restore the original admin.
    pub fn action_transfer_vault_authority(&mut self) -> bool {
        let transfer = roshi_client::instruction::transfer_vault_authority(
            self.operator.pubkey(),
            self.vault,
            self.vault_authority_alt.pubkey(),
        )
        .unwrap();
        if !submit(&mut self.ctx, transfer, &[&self.operator.clone()]) {
            return false;
        }

        let stale = roshi_client::instruction::transfer_vault_authority(
            self.operator.pubkey(),
            self.vault,
            self.operator.pubkey(),
        )
        .unwrap();
        let stale_ok = submit(&mut self.ctx, stale, &[&self.operator.clone()]);
        fuzz_assert!(
            !stale_ok,
            "old vault admin transferred authority after rotation"
        );

        let restore = roshi_client::instruction::transfer_vault_authority(
            self.vault_authority_alt.pubkey(),
            self.vault,
            self.operator.pubkey(),
        )
        .unwrap();
        submit(&mut self.ctx, restore, &[&self.vault_authority_alt.clone()])
    }

    /// Rotate strategist, prove the old strategist cannot execute a manager
    /// action, then restore the original strategist.
    pub fn action_set_strategist_authority(&mut self) -> bool {
        let set = roshi_client::instruction::set_strategist(
            self.operator.pubkey(),
            self.vault,
            self.strategist_alt.pubkey(),
        )
        .unwrap();
        if !submit(&mut self.ctx, set, &[&self.operator.clone()]) {
            return false;
        }

        if !self.load_vault().manage_paused().unwrap_or(true) {
            let ix = self.manage_transfer_ix(self.manage_action, self.external_account, 0);
            let old_ok = submit(&mut self.ctx, ix, &[&self.strategist.clone()]);
            fuzz_assert!(
                !old_ok,
                "old strategist executed manager action after rotation"
            );
        }

        let restore = roshi_client::instruction::set_strategist(
            self.operator.pubkey(),
            self.vault,
            self.strategist.pubkey(),
        )
        .unwrap();
        submit(&mut self.ctx, restore, &[&self.operator.clone()])
    }

    /// Rotate swap authority, prove the old swap authority cannot execute a
    /// swap, then restore the original swap authority.
    pub fn action_set_swap_authority(&mut self) -> bool {
        let set = roshi_client::instruction::set_swap_authority(
            self.operator.pubkey(),
            self.vault,
            self.swap_authority_alt.pubkey(),
        )
        .unwrap();
        if !submit(&mut self.ctx, set, &[&self.operator.clone()]) {
            return false;
        }

        if !self.load_vault().manage_paused().unwrap_or(true) {
            let ix = self.swap_base_ix(self.swap_authority.pubkey(), false, 0);
            let old_ok = submit(&mut self.ctx, ix, &[&self.swap_authority.clone()]);
            fuzz_assert!(
                !old_ok,
                "old swap authority executed swap after rotation"
            );
        }

        let restore = roshi_client::instruction::set_swap_authority(
            self.operator.pubkey(),
            self.vault,
            self.swap_authority.pubkey(),
        )
        .unwrap();
        submit(&mut self.ctx, restore, &[&self.operator.clone()])
    }

    /// Rotate NAV authority, prove the old NAV authority cannot report, then
    /// restore the original NAV authority.
    pub fn action_set_nav_authority(&mut self) -> bool {
        let set = roshi_client::instruction::set_nav_authority(
            self.operator.pubkey(),
            self.vault,
            self.nav_authority_alt.pubkey(),
        )
        .unwrap();
        if !submit(&mut self.ctx, set, &[&self.operator.clone()]) {
            return false;
        }

        self.report_nonce += 1;
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&self.report_nonce.to_le_bytes());
        let stale = roshi_client::instruction::report_nav(
            self.nav_authority.pubkey(),
            self.vault,
            self.share_mint,
            self.base_mint,
            self.custody,
            self.withdraw_custody,
            0,
            hash,
        )
        .unwrap();
        let old_ok = submit(&mut self.ctx, stale, &[&self.nav_authority.clone()]);
        fuzz_assert!(!old_ok, "old NAV authority reported after rotation");

        let restore = roshi_client::instruction::set_nav_authority(
            self.operator.pubkey(),
            self.vault,
            self.nav_authority.pubkey(),
        )
        .unwrap();
        submit(&mut self.ctx, restore, &[&self.operator.clone()])
    }

    /// Rotate withdrawal authority, prove the old withdrawal authority cannot
    /// settle even an empty batch, then restore the original authority.
    pub fn action_set_withdrawal_authority(&mut self) -> bool {
        let set = roshi_client::instruction::set_withdrawal_authority(
            self.operator.pubkey(),
            self.vault,
            self.withdrawal_authority_alt.pubkey(),
        )
        .unwrap();
        if !submit(&mut self.ctx, set, &[&self.operator.clone()]) {
            return false;
        }

        let stale = roshi_client::instruction::process_withdrawals(
            self.withdrawal_authority.pubkey(),
            self.vault,
            self.withdraw_sub_account,
            self.withdraw_custody,
            self.share_mint,
            Vec::new(),
        )
        .unwrap();
        let old_ok = submit(&mut self.ctx, stale, &[&self.withdrawal_authority.clone()]);
        fuzz_assert!(
            !old_ok,
            "old withdrawal authority processed withdrawals after rotation"
        );

        let restore = roshi_client::instruction::set_withdrawal_authority(
            self.operator.pubkey(),
            self.vault,
            self.withdrawal_authority.pubkey(),
        )
        .unwrap();
        submit(&mut self.ctx, restore, &[&self.operator.clone()])
    }

    /// Update mutable vault config through the admin path, while keeping the
    /// custody indices and treasury aligned with the fixture's canonical
    /// accounts. Then prove invalid BPS and non-admin updates leave state
    /// untouched.
    pub fn action_update_vault_config(
        &mut self,
        performance_fee_bps: u16,
        withdrawal_buffer_bps: u16,
        external_enabled: bool,
    ) -> bool {
        let performance_fee_bps = performance_fee_bps % (MAX_BPS + 1);
        let withdrawal_buffer_bps = withdrawal_buffer_bps % (MAX_BPS + 1);
        let args = UpdateVaultConfigArgs {
            treasury: self.treasury.to_bytes(),
            deposit_sub_account: 0,
            withdraw_sub_account: 1,
            base_oracle: OracleConfig::default(),
            performance_fee_bps,
            withdrawal_buffer_bps,
            controls: VaultControls::default(),
            external_enabled,
        };
        let ix =
            roshi_client::instruction::update_vault_config(self.operator.pubkey(), self.vault, args)
                .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&self.operator.clone()]);
        fuzz_assert!(ok, "valid update_vault_config rejected");

        let updated = self.load_vault();
        let updated_external_enabled = match updated.external_enabled() {
            Ok(flag) => flag,
            Err(err) => {
                fuzz_assert!(false, "external_enabled flag invalid after config update: {err:?}");
                false
            }
        };
        fuzz_assert!(
            updated.treasury == self.treasury.to_bytes()
                && updated.deposit_sub_account == 0
                && updated.withdraw_sub_account == 1
                && updated.performance_fee_bps == performance_fee_bps
                && updated.withdrawal_buffer_bps == withdrawal_buffer_bps
                && updated_external_enabled == external_enabled,
            "update_vault_config stored wrong config"
        );

        let before_invalid = self.load_vault();
        let invalid_args = UpdateVaultConfigArgs {
            treasury: self.treasury.to_bytes(),
            deposit_sub_account: 0,
            withdraw_sub_account: 1,
            base_oracle: OracleConfig::default(),
            performance_fee_bps: MAX_BPS + 1,
            withdrawal_buffer_bps,
            controls: VaultControls::default(),
            external_enabled,
        };
        let invalid_ix = roshi_client::instruction::update_vault_config(
            self.operator.pubkey(),
            self.vault,
            invalid_args,
        )
        .unwrap();
        let invalid_ok = submit(&mut self.ctx, invalid_ix, &[&self.operator.clone()]);
        let after_invalid = self.load_vault();
        fuzz_assert!(
            !invalid_ok && after_invalid == before_invalid,
            "invalid-BPS update_vault_config succeeded or mutated state"
        );

        let stale_args = UpdateVaultConfigArgs {
            treasury: self.treasury.to_bytes(),
            deposit_sub_account: 0,
            withdraw_sub_account: 1,
            base_oracle: OracleConfig::default(),
            performance_fee_bps,
            withdrawal_buffer_bps,
            controls: VaultControls::default(),
            external_enabled: !external_enabled,
        };
        let stale_ix = roshi_client::instruction::update_vault_config(
            self.external_authority.pubkey(),
            self.vault,
            stale_args,
        )
        .unwrap();
        let stale_ok = submit(&mut self.ctx, stale_ix, &[&self.external_authority.clone()]);
        let after_stale = self.load_vault();
        fuzz_assert!(
            !stale_ok && after_stale == before_invalid,
            "non-admin update_vault_config succeeded or mutated state"
        );

        true
    }

    /// Flip pause flags.
    pub fn action_set_pause_flags(
        &mut self,
        deposits: bool,
        withdrawals: bool,
        manage: bool,
    ) -> bool {
        let ix = roshi_client::instruction::set_pause_flags(
            self.operator.pubkey(),
            self.vault,
            deposits,
            withdrawals,
            manage,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.operator.clone()])
    }

    /// Advance the clock so time-dependent paths (fees, reporting, oracle
    /// staleness) are reachable. LiteSVM's `warp_to_slot` moves `Clock.slot`
    /// only; `unix_timestamp` starts at 0 and never advances on its own, which
    /// would leave the oracle staleness check (`publish_time + max_age < now`)
    /// permanently unreachable. Advance wall time alongside slots — sysvars are
    /// restored every fuzz iteration, so this never leaks across sequences.
    pub fn action_advance_slots(&mut self, #[range(0..32)] slots: u64) -> bool {
        let advanced = slots + 1;
        let target = self.ctx.slot() + advanced;
        self.ctx.warp_to_slot(target);
        let mut clock: Clock = self.ctx.svm.get_sysvar();
        clock.unix_timestamp += advanced as i64 * SECONDS_PER_SLOT;
        self.ctx.set_sysvar(&clock);
        true
    }
