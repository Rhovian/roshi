/// Swap the registered asset between two sub-account custodies while Scope
/// prices both endpoints. The routed base is either Pyth (mixed-provider
/// boundary) or the same Scope feed. A same-feed policy mismatch must reject
/// before the CPI can move tokens.
pub fn action_swap_scope_asset(
    &mut self,
    reverse: bool,
    amount: u64,
    scope_base: bool,
    mismatched_policy: bool,
) -> bool {
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

    let original_vault = self.load_vault();
    let policy_mismatch = scope_base && mismatched_policy;
    let asset_max_age = if policy_mismatch {
        SCOPE_MAX_AGE_SECS + 1
    } else {
        SCOPE_MAX_AGE_SECS
    };
    fuzz_assert!(
        self.update_primary_asset_oracle(
            self.scope_config_with_max_age(SCOPE_PRICE_INDEX, asset_max_age),
            true,
        ),
        "valid routed Scope asset config rejected"
    );

    let base_oracle = if scope_base {
        self.scope_config(SCOPE_PRICE_INDEX)
    } else {
        Self::pyth_config()
    };
    let controls = original_vault.controls;
    let swap_controls = VaultControls::new(
        controls.max_unlock_duration_secs,
        controls.max_report_age_secs,
        controls.min_report_interval_secs,
        controls.cancel_grace_slots,
        controls.max_nav_gain_bps,
        controls.atomic_redeem_fee_bps,
        100,
    );
    fuzz_assert!(
        self.update_vault_pricing(base_oracle, swap_controls),
        "failed to enable Scope swap valuation"
    );

    let (input, output, action) = if reverse {
        (
            self.asset_swap_custody,
            self.asset_custody,
            self.asset_swap_reverse_action,
        )
    } else {
        (
            self.asset_custody,
            self.asset_swap_custody,
            self.asset_swap_forward_action,
        )
    };
    let available = token_balance(&self.ctx.svm, &input);
    if available == 0 {
        fuzz_assert!(
            self.update_vault_pricing(original_vault.base_oracle, original_vault.controls)
                && self.restore_primary_asset_pyth(),
            "failed to restore oracle state after empty Scope swap"
        );
        return false;
    }
    let amount = (amount % available) + 1;

    let mut valuation_accounts = vec![
        AccountMeta::new_readonly(self.asset_pda, false),
        AccountMeta::new_readonly(self.scope_prices_account, false),
        AccountMeta::new_readonly(self.scope_mappings_account, false),
        AccountMeta::new_readonly(self.asset_pda, false),
        AccountMeta::new_readonly(self.scope_prices_account, false),
        AccountMeta::new_readonly(self.scope_mappings_account, false),
    ];
    if scope_base {
        valuation_accounts.extend([
            AccountMeta::new_readonly(self.scope_prices_account, false),
            AccountMeta::new_readonly(self.scope_mappings_account, false),
        ]);
    } else {
        valuation_accounts.push(AccountMeta::new_readonly(self.pyth_account, false));
    }

    let mut ix_data = vec![SPL_TRANSFER_TAG];
    ix_data.extend_from_slice(&amount.to_le_bytes());
    let ix = roshi_client::instruction::swap(
        self.strategist.pubkey(),
        self.vault,
        self.sub_account,
        input,
        output,
        action,
        valuation_accounts,
        vec![
            AccountMeta::new(input, false),
            AccountMeta::new(output, false),
            AccountMeta::new_readonly(self.sub_account, false),
            AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
        ],
        SwapArgs {
            min_out: amount,
            max_in: amount,
            sub_account: 0,
            accounts_start: 0,
            account_flags: PackedAccountFlags::from_flags(&[
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: false,
                },
            ]),
            ix_data,
        },
    )
    .unwrap();

    let input_before = token_balance(&self.ctx.svm, &input);
    let output_before = token_balance(&self.ctx.svm, &output);
    let manage_paused = original_vault
        .manage_paused()
        .expect("loaded vault has valid flags");
    let ok = submit(&mut self.ctx, ix, &[&self.strategist.clone()]);
    let input_after = token_balance(&self.ctx.svm, &input);
    let output_after = token_balance(&self.ctx.svm, &output);

    fuzz_assert!(
        self.update_vault_pricing(original_vault.base_oracle, original_vault.controls)
            && self.restore_primary_asset_pyth(),
        "failed to restore oracle state after Scope swap"
    );

    if policy_mismatch || manage_paused {
        fuzz_assert!(
            !ok && input_after == input_before && output_after == output_before,
            "rejected Scope swap moved tokens: ok={ok}, mismatch={policy_mismatch}, paused={manage_paused}, input {input_before}->{input_after}, output {output_before}->{output_after}"
        );
    } else {
        fuzz_assert!(
            ok && input_after == input_before - amount && output_after == output_before + amount,
            "valid Scope swap rejected or moved wrong amount: ok={ok}, input {input_before}->{input_after}, output {output_before}->{output_after}, amount={amount}"
        );
    }
    ok
}
