    /// Move idle custody base out to the external venue.
    pub fn action_invest_external(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let ix = roshi_client::instruction::invest_external(
            self.strategist.pubkey(),
            self.vault,
            0,
            self.sub_account,
            self.custody,
            self.external_account,
            self.external_destination,
            amount,
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.strategist.clone()])
    }

    /// Return base from the external venue back into custody.
    pub fn action_return_external(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.external_account);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let ix = roshi_client::instruction::return_external(
            self.strategist.pubkey(),
            self.external_authority.pubkey(),
            self.vault,
            0,
            self.sub_account,
            self.external_account,
            self.custody,
            amount,
        )
        .unwrap();
        let strategist = self.strategist.clone();
        let ext = self.external_authority.clone();
        submit(&mut self.ctx, ix, &[&strategist, &ext])
    }

    /// Build a `manage` instruction that CPIs an SPL token transfer of `amount`
    /// from custody to `destination`, signed by the sub-account PDA, against the
    /// pre-authorized `action`. The recomputed action hash matches only when
    /// `(action, destination)` are a pinned pair (e.g. `manage_action` with
    /// `external_account`); any mismatch — wrong destination, or a revoked
    /// action whose account is closed — must reject.
    fn manage_transfer_ix(
        &self,
        action: Pubkey,
        destination: Pubkey,
        amount: u64,
    ) -> solana_instruction::Instruction {
        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&amount.to_le_bytes());
        roshi_client::instruction::manage(
            self.strategist.pubkey(),
            self.vault,
            self.sub_account,
            action,
            vec![
                AccountMeta::new(self.custody, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            ],
            ManageArgs {
                sub_account: 0,
                accounts_start: 0,
                accounts_len: 3,
                account_flags: vec![
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
                ],
                ix_data,
            },
        )
        .unwrap()
    }

    /// Execute the authorized manager transfer (custody -> external) through the
    /// CPI authorization machinery. Conservation still holds — this just reaches
    /// the same custody/external move via `manage` rather than `invest_external`,
    /// exercising `validate_authorized_cpi` + `invoke_signed` with the real PDA.
    pub fn action_manage_transfer(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let ix = self.manage_transfer_ix(self.manage_action, self.external_account, amount);
        submit(&mut self.ctx, ix, &[&self.strategist.clone()])
    }

    /// Rebalance idle base from deposit custody to withdraw custody through the
    /// same authorized Manager CPI surface. This makes the split-custody
    /// withdrawal path live: `report_nav` counts both custodies, while
    /// `process_withdrawals` can only pay from the withdraw side.
    pub fn action_rebalance_to_withdraw(&mut self, amount: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let amount = (amount % available) + 1;
        let deposit_before = token_balance(&self.ctx.svm, &self.custody);
        let withdraw_before = token_balance(&self.ctx.svm, &self.withdraw_custody);
        let should_succeed = !self.load_vault().manage_paused().unwrap_or(true);
        let ix =
            self.manage_transfer_ix(self.rebalance_to_withdraw_action, self.withdraw_custody, amount);
        let ok = submit(&mut self.ctx, ix, &[&self.strategist.clone()]);
        let deposit_after = token_balance(&self.ctx.svm, &self.custody);
        let withdraw_after = token_balance(&self.ctx.svm, &self.withdraw_custody);
        if should_succeed {
            fuzz_assert!(
                ok && deposit_after == deposit_before - amount
                    && withdraw_after == withdraw_before + amount,
                "rebalance to withdraw custody rejected or moved wrong amount: \
                 ok={ok}, deposit {deposit_before}->{deposit_after}, withdraw {withdraw_before}->{withdraw_after}, amount={amount}"
            );
        }
        ok
    }

    /// Reuse the authorized action PDA but swap the CPI destination to a user
    /// ATA the action never pinned. The recomputed hash must not match, so the
    /// program must reject it: no funds may leave custody. Conservation alone
    /// cannot see this (the user ATA is tracked), so assert it directly.
    pub fn action_manage_tampered_destination(
        &mut self,
        #[range(0..NUM_USERS)] user: usize,
        amount: u64,
    ) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let destination = self.users[user].base_ata;
        let custody_before = token_balance(&self.ctx.svm, &self.custody);
        // 1..=available: a real transfer attempt, so a successful (buggy) move
        // would be observable rather than a no-op zero transfer.
        let amount = (amount % available) + 1;
        let ix = self.manage_transfer_ix(self.manage_action, destination, amount);
        let moved = submit(&mut self.ctx, ix, &[&self.strategist.clone()]);
        let custody_after = token_balance(&self.ctx.svm, &self.custody);
        fuzz_assert!(
            !moved && custody_after == custody_before,
            "unauthorized manage moved custody funds to an unpinned destination: \
             custody {custody_before} -> {custody_after} (success={moved})"
        );
        moved
    }

    /// Drive `revoke_action` and its security guarantee. If the revocable Manager
    /// action is currently authorized, revoke it (admin signs), then attempt a
    /// `manage` against the now-closed action and assert it moves no custody
    /// funds — proving revocation removes authority (conservation can't see this:
    /// the would-be destination, treasury, is tracked). If it's already revoked,
    /// re-authorize it (same accounts → same hash/PDA) so the next call can
    /// revoke again.
    pub fn action_revoke_action(&mut self) -> bool {
        let authorized = self
            .ctx
            .get_account(&self.revocable_action)
            .map(|a| a.owner == self.program_id && !a.data.is_empty())
            .unwrap_or(false);

        if !authorized {
            let operator = self.operator.clone();
            let (action, _) = authorize_transfer_action(
                &mut self.ctx,
                &operator,
                self.vault,
                self.sub_account,
                self.custody,
                self.treasury,
                ActionScope::Manager,
            );
            debug_assert_eq!(action, self.revocable_action);
            return true;
        }

        let revoke = roshi_client::instruction::revoke_action(
            self.operator.pubkey(),
            self.vault,
            self.revocable_action,
            self.revocable_action_hash,
        )
        .unwrap();
        if !submit(&mut self.ctx, revoke, &[&self.operator.clone()]) {
            return false;
        }

        // The action is closed now: a manage against it must reject before any
        // transfer, leaving custody untouched. The check is only non-vacuous when
        // a *still-authorized* action could actually move funds — i.e. custody
        // holds at least the 1 atom we try to transfer and manage isn't paused.
        // Otherwise a broken revocation would be masked by insufficient-funds or
        // the pause gate, so skip the assertion (the revoke itself still ran).
        let custody_before = token_balance(&self.ctx.svm, &self.custody);
        if custody_before == 0 || self.load_vault().manage_paused().unwrap_or(true) {
            return true;
        }
        let ix = self.manage_transfer_ix(self.revocable_action, self.treasury, 1);
        let moved = submit(&mut self.ctx, ix, &[&self.strategist.clone()]);
        let custody_after = token_balance(&self.ctx.svm, &self.custody);
        fuzz_assert!(
            !moved && custody_after == custody_before,
            "revoked action still moved custody funds: \
             {custody_before} -> {custody_after} (success={moved})"
        );
        true
    }

    /// Run two authorized custody -> external transfers in one `ManageBatch`.
    /// Both legs reuse the single authorized manage action (same accounts and
    /// discriminator hash to the same Action), so this exercises the batch
    /// loader's per-action `(sub_account, action)` pair loop and the
    /// per-action `accounts_start` slicing of the shared CPI account section.
    /// The second leg is sized to what the first leaves, so the batch settles.
    pub fn action_manage_batch(&mut self, amount_a: u64, amount_b: u64) -> bool {
        let available = token_balance(&self.ctx.svm, &self.custody);
        if available == 0 {
            return false;
        }
        let amount1 = amount_a % (available + 1);
        let remaining = available - amount1;
        let amount2 = amount_b % (remaining + 1);

        let mut ix_data_1 = vec![SPL_TRANSFER_TAG];
        ix_data_1.extend_from_slice(&amount1.to_le_bytes());
        let mut ix_data_2 = vec![SPL_TRANSFER_TAG];
        ix_data_2.extend_from_slice(&amount2.to_le_bytes());

        let pair = roshi_client::instruction::ManageBatchActionAccounts {
            sub_account_pda: self.sub_account,
            action: self.manage_action,
        };
        let transfer_flags = || {
            vec![
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
            ]
        };
        let leg = |start: u8, ix_data: Vec<u8>| ManageArgs {
            sub_account: 0,
            accounts_start: start,
            accounts_len: 3,
            account_flags: transfer_flags(),
            ix_data,
        };
        // Shared CPI section: each leg's 3 metas immediately followed by its CPI
        // program account, so leg 0 selects [0,3) (program at 3) and leg 1
        // selects [4,7) (program at 7).
        let cpi_accounts = vec![
            AccountMeta::new(self.custody, false),
            AccountMeta::new(self.external_account, false),
            AccountMeta::new_readonly(self.sub_account, false),
            AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            AccountMeta::new(self.custody, false),
            AccountMeta::new(self.external_account, false),
            AccountMeta::new_readonly(self.sub_account, false),
            AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
        ];
        let ix = roshi_client::instruction::manage_batch(
            self.strategist.pubkey(),
            self.vault,
            vec![pair, pair],
            cpi_accounts,
            vec![leg(0, ix_data_1), leg(4, ix_data_2)],
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&self.strategist.clone()])
    }

    /// Execute an authorized base->base swap between the two sub-account
    /// custodies. Degenerate as a swap, but exercises all of `try_swap`: the
    /// realized input/output bounds, custody reverification, and the signed CPI.
    /// `reverse` picks the direction so base is never one-way stranded.
    fn swap_base_ix(
        &self,
        strategist: Pubkey,
        reverse: bool,
        amount: u64,
    ) -> solana_instruction::Instruction {
        let (input, output, action) = if reverse {
            (self.swap_custody, self.custody, self.swap_reverse_action)
        } else {
            (self.custody, self.swap_custody, self.swap_forward_action)
        };
        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&amount.to_le_bytes());
        roshi_client::instruction::swap(
            strategist,
            self.vault,
            self.sub_account,
            input,
            output,
            action,
            vec![],
            vec![
                AccountMeta::new(input, false),
                AccountMeta::new(output, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            ],
            SwapArgs {
                // The transfer moves exactly `amount`, so spent == received ==
                // amount: within max_in and at/above min_out by construction.
                min_out: 0,
                max_in: amount,
                sub_account: 0,
                accounts_start: 0,
                accounts_len: 3,
                account_flags: vec![
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
                ],
                ix_data,
            },
        )
        .unwrap()
    }

    pub fn action_swap(&mut self, reverse: bool, amount: u64) -> bool {
        let input = if reverse {
            self.swap_custody
        } else {
            self.custody
        };
        let available = token_balance(&self.ctx.svm, &input);
        if available == 0 {
            return false;
        }
        let amount = amount % (available + 1);
        let ix = self.swap_base_ix(self.strategist.pubkey(), reverse, amount);
        submit(&mut self.ctx, ix, &[&self.strategist.clone()])
    }

    /// Execute an authorized Token-2022 swap between two sub-account-owned
    /// custodies for the registered bare Token-2022 asset. This mirrors
    /// `action_swap`, but pins the CPI to the Token-2022 program id and asserts
    /// exact Token-2022 atom movement when managing is enabled.
    pub fn action_swap_token_2022_asset(&mut self, reverse: bool, amount: u64) -> bool {
        let (input, output, action) = if reverse {
            (
                self.token_2022_swap_custody,
                self.token_2022_asset_custody,
                self.token_2022_swap_reverse_action,
            )
        } else {
            (
                self.token_2022_asset_custody,
                self.token_2022_swap_custody,
                self.token_2022_swap_forward_action,
            )
        };
        let available = token_balance(&self.ctx.svm, &input);
        if available == 0 {
            return false;
        }
        let amount = (amount % available) + 1;
        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&amount.to_le_bytes());
        let input_before = token_balance(&self.ctx.svm, &input);
        let output_before = token_balance(&self.ctx.svm, &output);
        let should_succeed = !self.load_vault().manage_paused().unwrap_or(true);
        let ix = roshi_client::instruction::swap(
            self.strategist.pubkey(),
            self.vault,
            self.sub_account,
            input,
            output,
            action,
            vec![],
            vec![
                AccountMeta::new(input, false),
                AccountMeta::new(output, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_2022_PROGRAM_ID, false),
            ],
            SwapArgs {
                min_out: amount,
                max_in: amount,
                sub_account: 0,
                accounts_start: 0,
                accounts_len: 3,
                account_flags: vec![
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
                ],
                ix_data,
            },
        )
        .unwrap();
        let ok = submit(&mut self.ctx, ix, &[&self.strategist.clone()]);
        let input_after = token_balance(&self.ctx.svm, &input);
        let output_after = token_balance(&self.ctx.svm, &output);
        if should_succeed {
            fuzz_assert!(
                ok && input_after == input_before - amount
                    && output_after == output_before + amount,
                "Token-2022 swap rejected or moved wrong amount: \
                 ok={ok}, input {input_before}->{input_after}, output {output_before}->{output_after}, amount={amount}"
            );
        }
        ok
    }

    /// Redeem shares synchronously through the authorized unwind CPI: pull base
    /// from the venue into custody, pay the owner's recipient, and burn the
    /// shares. Exercises all of `try_atomic_redeem` — the share-balance and
    /// entitlement bounds, the unwind-into-custody check, payout, and burn. The
    /// unwind amount is sized to the on-chain entitlement (recomputed here with
    /// the same `assets_for_redeem`) and capped by the venue balance, so the
    /// redeem clears its own bounds.
    pub fn action_atomic_redeem(&mut self, #[range(0..NUM_USERS)] user: usize, shares: u64) -> bool {
        let user = self.users[user].clone();
        let share_balance = token_balance(&self.ctx.svm, &user.share_ata);
        if share_balance == 0 {
            return false;
        }
        let shares = (shares % share_balance) + 1;

        // Entitlement at the current NAV, recomputed exactly as the program does.
        let account = self.ctx.get_account(&self.vault).expect("vault exists");
        let vault = Vault::from_account_data(&account.data).expect("vault decodes");
        let supply = mint_supply(&self.ctx.svm, &self.share_mint);
        let Some(economic) = supply.checked_add(vault.requested_withdrawal_shares) else {
            return false;
        };
        let Ok(effective) = vault.effective_total_assets(self.unix_timestamp()) else {
            fuzz_assert!(false, "effective NAV failed before atomic redeem");
            return false;
        };
        let Ok(max_owed) = assets_for_redeem(shares, effective, economic, BASE_DECIMALS) else {
            // Zero/invalid entitlement (e.g. nothing deposited yet): nothing to do.
            return false;
        };
        let unwind = max_owed.min(token_balance(&self.ctx.svm, &self.atomic_venue));
        if unwind == 0 {
            return false;
        }

        let mut ix_data = vec![SPL_TRANSFER_TAG];
        ix_data.extend_from_slice(&unwind.to_le_bytes());
        let ix = roshi_client::instruction::atomic_redeem(
            user.kp.pubkey(),
            self.vault,
            user.share_ata,
            self.share_mint,
            user.base_ata,
            self.custody,
            support::TOKEN_PROGRAM_ID,
            self.sub_account,
            self.atomic_action,
            vec![
                AccountMeta::new(self.atomic_venue, false),
                AccountMeta::new(self.custody, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(support::TOKEN_PROGRAM_ID, false),
            ],
            AtomicRedeemArgs {
                shares,
                min_output: 0,
                sub_account: 0,
                accounts_start: 0,
                accounts_len: 3,
                account_flags: vec![
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
                ],
                ix_data,
            },
        )
        .unwrap();
        submit(&mut self.ctx, ix, &[&user.kp])
    }
