    pub fn setup() -> Self {
        let mut ctx = TestContext::new();
        let program_id = ID;
        ctx.add_program(&program_id, "../target/deploy/roshi.so")
            .expect("load roshi.so (run `just build` first)");

        let operator = Rc::new(Keypair::new());
        let program_authority_alt = Rc::new(Keypair::new());
        let vault_authority_alt = Rc::new(Keypair::new());
        let strategist = Rc::new(Keypair::new());
        let strategist_alt = Rc::new(Keypair::new());
        let nav_authority = Rc::new(Keypair::new());
        let nav_authority_alt = Rc::new(Keypair::new());
        let withdrawal_authority = Rc::new(Keypair::new());
        let withdrawal_authority_alt = Rc::new(Keypair::new());
        let external_authority = Rc::new(Keypair::new());
        for payer in [
            &operator,
            &program_authority_alt,
            &vault_authority_alt,
            &strategist,
            &strategist_alt,
            &nav_authority,
            &nav_authority_alt,
            &withdrawal_authority,
            &withdrawal_authority_alt,
            &external_authority,
        ] {
            ctx.svm.airdrop(&payer.pubkey(), FUND_LAMPORTS).unwrap();
        }

        // 1. Program config.
        let (config_pda, _) = ProgramConfig::find_address();
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_program(
                operator.pubkey(),
                config_pda,
                operator.pubkey(),
            )
            .unwrap(),
            &[&operator],
            "initialize_program",
        );

        // 2. Vault. Deposit and withdraw custody are intentionally split across
        //    sub-accounts 0 and 1. A strategist rebalance action below moves
        //    idle base to the withdraw side so report/settle exercises distinct
        //    custody ATAs.
        let base_mint = Pubkey::new_unique();
        let (vault, bump) = Vault::find_address(b"main", &base_mint).unwrap();
        let share_mint = find_share_mint_address(&vault).0;
        let treasury = Pubkey::new_unique();
        set_mint(&mut ctx.svm, base_mint, &vault, BASE_DECIMALS);
        set_token_account(&mut ctx.svm, treasury, &base_mint, &Pubkey::new_unique(), 0);

        let args = InitializeVaultArgs {
            tag: pad_tag(b"main"),
            tag_len: 4,
            admin: operator.pubkey().to_bytes(),
            strategist: strategist.pubkey().to_bytes(),
            nav_authority: nav_authority.pubkey().to_bytes(),
            withdrawal_authority: withdrawal_authority.pubkey().to_bytes(),
            base_mint: base_mint.to_bytes(),
            base_decimals: BASE_DECIMALS,
            base_oracle: OracleConfig::default(),
            deposit_sub_account: 0,
            withdraw_sub_account: 1,
            treasury: treasury.to_bytes(),
            performance_fee_bps: PERF_FEE_BPS,
            withdrawal_buffer_bps: WITHDRAWAL_BUFFER_BPS,
            controls: VaultControls::default(),
            private: false,
            access_merkle_root: [0; 32],
        };
        let _ = bump;
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_vault(
                operator.pubkey(),
                config_pda,
                operator.pubkey(),
                vault,
                args,
            )
            .unwrap(),
            &[&operator],
            "initialize_vault",
        );

        // 3. Enable external investing.
        submit_ok(
            &mut ctx,
            roshi_client::instruction::update_vault_config(
                operator.pubkey(),
                vault,
                UpdateVaultConfigArgs {
                    treasury: treasury.to_bytes(),
                    deposit_sub_account: 0,
                    withdraw_sub_account: 1,
                    base_oracle: OracleConfig::default(),
                    performance_fee_bps: PERF_FEE_BPS,
                    withdrawal_buffer_bps: WITHDRAWAL_BUFFER_BPS,
                    controls: VaultControls::default(),
                    external_enabled: true,
                },
            )
            .unwrap(),
            &[&operator],
            "update_vault_config",
        );

        // 4. Custody + external token accounts (base).
        let sub_account = VaultSubAccount::find_address(&vault, 0).0;
        let withdraw_sub_account = VaultSubAccount::find_address(&vault, 1).0;
        let custody = set_ata(&mut ctx.svm, &sub_account, &base_mint, 0);
        let withdraw_custody = set_ata(&mut ctx.svm, &withdraw_sub_account, &base_mint, 0);
        let external_account = set_ata(&mut ctx.svm, &external_authority.pubkey(), &base_mint, 0);

        // 4a. Register the external venue: `invest_external` only moves
        //     custody to admin-authorized destinations.
        let (external_destination, _) =
            ExternalDestination::find_address(&vault, &external_account);
        submit_ok(
            &mut ctx,
            roshi_client::instruction::register_external_destination(
                operator.pubkey(),
                vault,
                external_account,
                external_destination,
            )
            .unwrap(),
            &[&operator],
            "register_external_destination",
        );

        // 4b. Authorize one Manager action: an SPL token transfer custody ->
        //     external, signed by the sub-account PDA, amount free. The
        //     recomputed hash at `manage` time must match this — the authz path
        //     under test.
        let (manage_action, _) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            custody,
            external_account,
            ActionScope::Manager,
        );
        let (rebalance_to_withdraw_action, _) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            custody,
            withdraw_custody,
            ActionScope::Manager,
        );

        // 4c. Second base custody (the swap output leg) owned by the sub-account,
        //     plus a Swap action in each direction between it and the deposit
        //     custody. Drives `swap` end to end.
        let swap_custody = Pubkey::new_unique();
        set_token_account(&mut ctx.svm, swap_custody, &base_mint, &sub_account, 0);
        let (swap_forward_action, _) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            custody,
            swap_custody,
            ActionScope::Swap,
        );
        let (swap_reverse_action, _) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            swap_custody,
            custody,
            ActionScope::Swap,
        );

        // 4d. AtomicRedeem: a sub-account-owned venue account pre-funded with
        //     deployed capital, plus an AtomicRedeem action whose unwind CPI
        //     pulls base venue -> custody. The ops pin the two writable
        //     sub-account custodies the unwind touches (venue source, base
        //     destination) and the transfer discriminator, so a public caller
        //     cannot redirect the route to drain an unpinned sibling; the redeem
        //     is further bounded by the on-chain share entitlement. The unwind
        //     amount (ix_data[1..9]) stays free, and `redeem_amount_offset = 1`
        //     is where it sits in the token-transfer ix data ([tag, amount_le]).
        let atomic_venue = Pubkey::new_unique();
        set_token_account(&mut ctx.svm, atomic_venue, &base_mint, &sub_account, VENUE_BASE);
        let atomic_ops = Ops::new([
            Op::IngestAccount { index: 0 },
            Op::IngestAccount { index: 1 },
            Op::IngestAccount { index: 2 },
            Op::IngestInstruction { offset: 0, len: 1 },
        ])
        .expect("ops within capacity");
        let atomic_metas = vec![
            AccountMeta::new(atomic_venue, false),
            AccountMeta::new(custody, false),
            AccountMeta::new_readonly(sub_account, true),
        ];
        let atomic_action_hash = compute_action_hash_from_metas(
            &support::TOKEN_PROGRAM_ID,
            &atomic_ops,
            &atomic_metas,
            &[SPL_TRANSFER_TAG],
            &[],
        )
        .expect("action hash");
        let (atomic_action, _) = Action::find_address(&vault, &atomic_action_hash);
        submit_ok(
            &mut ctx,
            roshi_client::instruction::authorize_action(
                operator.pubkey(),
                vault,
                atomic_action,
                atomic_action_hash,
                ActionScope::AtomicRedeem,
                atomic_ops,
                1,
                0,
                0,
            )
            .unwrap(),
            &[&operator],
            "authorize_action(atomic_redeem)",
        );

        // 4e. A revocable Manager action (custody -> treasury) used only to drive
        //     `revoke_action`: action_revoke_action closes it and asserts a manage
        //     against it then moves no funds, then re-authorizes it. Distinct
        //     destination from manage_action so it gets its own Action PDA.
        let (revocable_action, revocable_action_hash) = authorize_transfer_action(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            custody,
            treasury,
            ActionScope::Manager,
        );

        // 4f. Register a non-base asset priced through a mock Pyth feed. The
        //     custody is the sub-account's ATA for the asset mint; the price
        //     account is installed fresh (publish_time == now == 0) so deposits
        //     price through `oracle.rs` from the first action. This exercises
        //     `initialize_asset` for real (admin-signed, PDA-funded).
        let asset_mint = Pubkey::new_unique();
        set_mint(&mut ctx.svm, asset_mint, &operator.pubkey(), ASSET_DECIMALS);
        let asset_custody = set_ata(&mut ctx.svm, &sub_account, &asset_mint, 0);
        let pyth_account = Pubkey::new_unique();
        set_pyth_price(
            &mut ctx.svm,
            pyth_account,
            PYTH_FEED_ID,
            PYTH_BASE_PRICE,
            0,
            PYTH_EXPONENT,
            0,
        );
        let (asset_pda, _) = Asset::find_address(&vault, &asset_mint);
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_asset(
                operator.pubkey(),
                vault,
                asset_mint,
                asset_pda,
                InitializeAssetArgs {
                    asset_mint: asset_mint.to_bytes(),
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
            .unwrap(),
            &[&operator],
            "initialize_asset",
        );

        // 4g. Register a bare Token-2022 asset. Extended Token-2022 mints are
        //     installed too but intentionally not registered; an action below
        //     asserts `initialize_asset` rejects them before creating the PDA.
        let token_2022_asset_mint = Pubkey::new_unique();
        set_token_2022_mint(
            &mut ctx.svm,
            token_2022_asset_mint,
            &operator.pubkey(),
            ASSET_DECIMALS,
        );
        let token_2022_asset_custody = set_ata_with_program(
            &mut ctx.svm,
            &sub_account,
            &token_2022_asset_mint,
            0,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let token_2022_swap_custody = Pubkey::new_unique();
        set_token_account_with_program(
            &mut ctx.svm,
            token_2022_swap_custody,
            &token_2022_asset_mint,
            &sub_account,
            0,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let (token_2022_swap_forward_action, _) = authorize_transfer_action_with_program(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            token_2022_asset_custody,
            token_2022_swap_custody,
            ActionScope::Swap,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let (token_2022_swap_reverse_action, _) = authorize_transfer_action_with_program(
            &mut ctx,
            &operator,
            vault,
            sub_account,
            token_2022_swap_custody,
            token_2022_asset_custody,
            ActionScope::Swap,
            support::TOKEN_2022_PROGRAM_ID,
        );
        let (token_2022_asset_pda, _) = Asset::find_address(&vault, &token_2022_asset_mint);
        submit_ok(
            &mut ctx,
            roshi_client::instruction::initialize_asset(
                operator.pubkey(),
                vault,
                token_2022_asset_mint,
                token_2022_asset_pda,
                InitializeAssetArgs {
                    asset_mint: token_2022_asset_mint.to_bytes(),
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
            .unwrap(),
            &[&operator],
            "initialize_asset(token_2022)",
        );

        let transfer_fee_token_2022_mint = Pubkey::new_unique();
        set_transfer_fee_token_2022_mint(
            &mut ctx.svm,
            transfer_fee_token_2022_mint,
            &operator.pubkey(),
            ASSET_DECIMALS,
        );
        let (transfer_fee_token_2022_asset_pda, _) =
            Asset::find_address(&vault, &transfer_fee_token_2022_mint);

        // 5. Users, each funded with base + the non-base asset; share ATA
        //    starts empty.
        let mut users = Vec::with_capacity(NUM_USERS);
        let mut base_accounts = vec![
            custody,
            withdraw_custody,
            swap_custody,
            atomic_venue,
            external_account,
            treasury,
        ];
        let mut asset_accounts = vec![asset_custody];
        let mut token_2022_asset_accounts = vec![token_2022_asset_custody, token_2022_swap_custody];
        for _ in 0..NUM_USERS {
            let kp = Rc::new(Keypair::new());
            ctx.svm.airdrop(&kp.pubkey(), FUND_LAMPORTS).unwrap();
            let base_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &base_mint, INITIAL_USER_BASE);
            let share_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &share_mint, 0);
            let asset_ata = set_ata(&mut ctx.svm, &kp.pubkey(), &asset_mint, INITIAL_USER_ASSET);
            let token_2022_asset_ata = set_ata_with_program(
                &mut ctx.svm,
                &kp.pubkey(),
                &token_2022_asset_mint,
                INITIAL_USER_ASSET,
                support::TOKEN_2022_PROGRAM_ID,
            );
            base_accounts.push(base_ata);
            asset_accounts.push(asset_ata);
            token_2022_asset_accounts.push(token_2022_asset_ata);
            users.push(FuzzUser {
                kp,
                base_ata,
                share_ata,
                asset_ata,
                token_2022_asset_ata,
                access_proof: Vec::new(),
            });
        }

        // 6. Whitelist every user in a real access tree and flip the vault
        //    private. Members deposit with their proofs; the core loop survives.
        let leaves: Vec<[u8; 32]> = users
            .iter()
            .map(|u| access_merkle_leaf(&u.kp.pubkey()))
            .collect();
        let (members_root, proofs) = build_access_tree(&leaves);
        for (user, proof) in users.iter_mut().zip(proofs) {
            // Fail loudly at setup if the builder and the program's verifier
            // disagree, rather than silently breaking every member deposit.
            assert!(
                verify_access_merkle_proof(&user.kp.pubkey(), &members_root, &proof),
                "access tree builder produced an invalid member proof"
            );
            user.access_proof = proof;
        }

        // An outsider absent from the tree, carrying a stolen member proof.
        let outsider_kp = Rc::new(Keypair::new());
        ctx.svm.airdrop(&outsider_kp.pubkey(), FUND_LAMPORTS).unwrap();
        let outsider_base = set_ata(&mut ctx.svm, &outsider_kp.pubkey(), &base_mint, INITIAL_USER_BASE);
        let outsider_share = set_ata(&mut ctx.svm, &outsider_kp.pubkey(), &share_mint, 0);
        let outsider_asset = set_ata(&mut ctx.svm, &outsider_kp.pubkey(), &asset_mint, 0);
        let outsider_token_2022_asset = set_ata_with_program(
            &mut ctx.svm,
            &outsider_kp.pubkey(),
            &token_2022_asset_mint,
            0,
            support::TOKEN_2022_PROGRAM_ID,
        );
        base_accounts.push(outsider_base);
        asset_accounts.push(outsider_asset);
        token_2022_asset_accounts.push(outsider_token_2022_asset);
        let outsider = FuzzUser {
            kp: outsider_kp,
            base_ata: outsider_base,
            share_ata: outsider_share,
            asset_ata: outsider_asset,
            token_2022_asset_ata: outsider_token_2022_asset,
            access_proof: users[0].access_proof.clone(),
        };

        submit_ok(
            &mut ctx,
            roshi_client::instruction::set_vault_access(operator.pubkey(), vault, true, members_root)
                .unwrap(),
            &[&operator],
            "set_vault_access",
        );

        let initial_base =
            (NUM_USERS + 1) as u128 * INITIAL_USER_BASE as u128 + VENUE_BASE as u128;
        let initial_asset = NUM_USERS as u128 * INITIAL_USER_ASSET as u128;
        let initial_token_2022_asset = NUM_USERS as u128 * INITIAL_USER_ASSET as u128;

        Self {
            ctx,
            program_id,
            config_pda,
            operator,
            program_authority_alt,
            vault_authority_alt,
            strategist,
            strategist_alt,
            nav_authority,
            nav_authority_alt,
            withdrawal_authority,
            withdrawal_authority_alt,
            external_authority,
            vault,
            share_mint,
            base_mint,
            treasury,
            sub_account,
            custody,
            withdraw_sub_account,
            withdraw_custody,
            external_account,
            external_destination,
            manage_action,
            rebalance_to_withdraw_action,
            swap_custody,
            swap_forward_action,
            swap_reverse_action,
            atomic_venue,
            atomic_action,
            revocable_action,
            revocable_action_hash,
            members_root,
            outsider,
            users,
            base_accounts,
            initial_base,
            asset_mint,
            asset_pda,
            asset_custody,
            pyth_account,
            asset_accounts,
            initial_asset,
            token_2022_asset_mint,
            token_2022_asset_pda,
            token_2022_asset_custody,
            token_2022_swap_custody,
            token_2022_swap_forward_action,
            token_2022_swap_reverse_action,
            transfer_fee_token_2022_mint,
            transfer_fee_token_2022_asset_pda,
            token_2022_asset_accounts,
            initial_token_2022_asset,
            report_nonce: 0,
            prev_high_watermark: 0,
        }
    }
