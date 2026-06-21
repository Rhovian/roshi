//! `swap`: strategist execution through a pre-authorized CPI with realized
//! balance bounds on the designated custody accounts.

use litesvm::LiteSVM;
use roshi::{
    error::RoshiError,
    instructions::{AccountFlags, SwapArgs},
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Op, Ops},
        sub_account::VaultSubAccount,
        Account as RoshiAccount,
    },
    ID,
};
use solana_instruction::{error::InstructionError, AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
use wincode::serialize;

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok,
    set_token_account_with_program, setup_program, token_balance, VaultBuilder,
    TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};

const INPUT_BALANCE: u64 = 1_000_000;
const OUTPUT_BALANCE: u64 = 100_000;
const SWAP_AMOUNT: u64 = 250_000;

struct SwapFixture {
    vault: crate::helpers::TestVault,
    sub_account_index: u8,
    sub_account: Pubkey,
    input_custody: Pubkey,
    output_custody: Pubkey,
    action_pda: Pubkey,
    action_hash: [u8; 32],
    ix_data: Vec<u8>,
    ops: Ops,
    token_program: Pubkey,
}

impl SwapFixture {
    fn setup(svm: &mut LiteSVM) -> Self {
        Self::setup_with_program(svm, TOKEN_PROGRAM_ID)
    }

    fn setup_with_program(svm: &mut LiteSVM, token_program: Pubkey) -> Self {
        let builder = VaultBuilder::new();
        builder.install_mints(svm);
        let vault = builder.install(svm);

        let sub_account_index = 0;
        let sub_account = VaultSubAccount::find_address(&vault.address, sub_account_index).0;
        let input_custody = Pubkey::new_unique();
        let output_custody = Pubkey::new_unique();
        set_token_account_with_program(
            svm,
            input_custody,
            &vault.base_mint,
            &sub_account,
            INPUT_BALANCE,
            token_program,
        );
        set_token_account_with_program(
            svm,
            output_custody,
            &vault.base_mint,
            &sub_account,
            OUTPUT_BALANCE,
            token_program,
        );

        let ix_data = token_transfer_data(SWAP_AMOUNT);
        let ops = Ops::new([
            Op::IngestAccount { index: 0 },
            Op::IngestAccount { index: 1 },
        ])
        .unwrap();
        let action_metas = token_transfer_metas(input_custody, output_custody, sub_account);
        let action_hash =
            compute_action_hash_from_metas(&token_program, &ops, &action_metas, &ix_data, &[])
                .unwrap();
        let action_pda = Action::find_address(&vault.address, &action_hash).0;

        Self {
            vault,
            sub_account_index,
            sub_account,
            input_custody,
            output_custody,
            action_pda,
            action_hash,
            ix_data,
            ops,
            token_program,
        }
    }

    fn install_action(&self, svm: &mut LiteSVM) {
        let (_, action_bump) = Action::find_address(&self.vault.address, &self.action_hash);
        svm.set_account(
            self.action_pda,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
                data: serialize(&RoshiAccount::Action(Action {
                    vault: self.vault.address.to_bytes(),
                    action_hash: self.action_hash,
                    ops: self.ops,
                    scope: ActionScope::Swap,
                    fee_num: 0,
                    fee_den: 0,
                    redeem_amount_offset: 0,
                    bump: action_bump,
                }))
                .unwrap(),
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }

    fn ix(&self, strategist: Pubkey, min_out: u64, max_in: u64) -> Instruction {
        roshi_client::instruction::swap(
            strategist,
            self.vault.address,
            self.sub_account,
            self.input_custody,
            self.output_custody,
            self.action_pda,
            vec![],
            vec![
                AccountMeta::new(self.input_custody, false),
                AccountMeta::new(self.output_custody, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(self.token_program, false),
            ],
            SwapArgs {
                min_out,
                max_in,
                sub_account: self.sub_account_index,
                program_id: self.token_program.to_bytes(),
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
                ix_data: self.ix_data.clone(),
            },
        )
        .unwrap()
    }
}

#[test]
fn test_swap_happy_path() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fixture.install_action(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    send_ok(
        &mut svm,
        fixture.ix(
            fixture.vault.roles.strategist.pubkey(),
            SWAP_AMOUNT,
            SWAP_AMOUNT,
        ),
        &fixture.vault.roles.strategist,
    );

    assert_eq!(
        token_balance(&svm, &fixture.input_custody),
        INPUT_BALANCE - SWAP_AMOUNT
    );
    assert_eq!(
        token_balance(&svm, &fixture.output_custody),
        OUTPUT_BALANCE + SWAP_AMOUNT
    );
}

#[test]
fn test_swap_happy_path_with_token_2022_custody() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup_with_program(&mut svm, TOKEN_2022_PROGRAM_ID);
    fixture.install_action(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    send_ok(
        &mut svm,
        fixture.ix(
            fixture.vault.roles.strategist.pubkey(),
            SWAP_AMOUNT,
            SWAP_AMOUNT,
        ),
        &fixture.vault.roles.strategist,
    );

    assert_eq!(
        token_balance(&svm, &fixture.input_custody),
        INPUT_BALANCE - SWAP_AMOUNT
    );
    assert_eq!(
        token_balance(&svm, &fixture.output_custody),
        OUTPUT_BALANCE + SWAP_AMOUNT
    );
}

#[test]
fn test_swap_rejects_received_below_min_out() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fixture.install_action(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(
                fixture.vault.roles.strategist.pubkey(),
                SWAP_AMOUNT + 1,
                SWAP_AMOUNT,
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::SlippageExceeded,
    );
}

#[test]
fn test_swap_rejects_spent_above_max_in() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fixture.install_action(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(
                fixture.vault.roles.strategist.pubkey(),
                SWAP_AMOUNT,
                SWAP_AMOUNT - 1,
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::SlippageExceeded,
    );
}

#[test]
fn test_swap_rejects_when_manage_paused() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fixture.install_action(&mut svm);
    fund(&mut svm, &fixture.vault.roles.admin);
    fund(&mut svm, &fixture.vault.roles.strategist);
    send_ok(
        &mut svm,
        roshi_client::instruction::set_pause_flags(
            fixture.vault.roles.admin.pubkey(),
            fixture.vault.address,
            false,
            false,
            true,
        )
        .unwrap(),
        &fixture.vault.roles.admin,
    );
    svm.expire_blockhash();

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(
                fixture.vault.roles.strategist.pubkey(),
                SWAP_AMOUNT,
                SWAP_AMOUNT,
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::VaultPaused,
    );
}

#[test]
fn test_swap_rejects_non_strategist_signer() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fixture.install_action(&mut svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    assert_instruction_error(
        send(
            &mut svm,
            fixture.ix(outsider.pubkey(), SWAP_AMOUNT, SWAP_AMOUNT),
            &outsider,
        ),
        InstructionError::IllegalOwner,
    );
}

#[test]
fn test_swap_rejects_custody_with_delegate() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fixture.install_action(&mut svm);
    install_delegate(&mut svm, fixture.input_custody);
    fund(&mut svm, &fixture.vault.roles.strategist);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(
                fixture.vault.roles.strategist.pubkey(),
                SWAP_AMOUNT,
                SWAP_AMOUNT,
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::InvalidTokenAccount,
    );
}

fn token_transfer_data(amount: u64) -> Vec<u8> {
    let mut data = vec![3];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

fn token_transfer_metas(
    source: Pubkey,
    destination: Pubkey,
    authority: Pubkey,
) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(source, false),
        AccountMeta::new(destination, false),
        AccountMeta::new_readonly(authority, true),
    ]
}

fn install_delegate(svm: &mut LiteSVM, address: Pubkey) {
    let mut account = svm.get_account(&address).unwrap();
    account.data[72..76].copy_from_slice(&1u32.to_le_bytes());
    account.data[76..108].copy_from_slice(Pubkey::new_unique().as_ref());
    svm.set_account(address, account).unwrap();
}

// --- Oracle-bounded swap slippage (`max_swap_slippage_bps`) ---

fn set_swap_slippage(svm: &mut LiteSVM, fixture_vault: &crate::helpers::TestVault, bps: u16) {
    let mut state = fixture_vault.load(svm);
    state.controls = roshi::state::vault::VaultControls::new(0, 0, 0, 0, 0, 0, bps);
    svm.set_account(
        fixture_vault.address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(roshi::state::vault::Vault::SPACE),
            data: serialize(&RoshiAccount::Vault(state)).unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

/// Install a registered Asset directly (9 decimals, enabled, uncapped).
fn install_asset(
    svm: &mut LiteSVM,
    vault: &crate::helpers::TestVault,
    mint: Pubkey,
    oracle: roshi::oracle::OracleConfig,
    routed: bool,
) -> Pubkey {
    let (pda, bump) = roshi::state::asset::Asset::find_address(&vault.address, &mint);
    let asset = roshi::state::asset::Asset::new(
        vault.address.to_bytes(),
        mint.to_bytes(),
        oracle,
        9,
        true,
        routed,
        u64::MAX,
        bump,
    )
    .unwrap();
    svm.set_account(
        pda,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(roshi::state::asset::Asset::SPACE),
            data: serialize(&RoshiAccount::Asset(asset)).unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    pda
}

/// Install a Swap-scope action for one SPL transfer `(from -> to)` signed by
/// the fixture sub-account, returning the action PDA.
fn install_transfer_action(
    svm: &mut LiteSVM,
    fixture: &SwapFixture,
    from: Pubkey,
    to: Pubkey,
) -> Pubkey {
    let metas = token_transfer_metas(from, to, fixture.sub_account);
    let hash = compute_action_hash_from_metas(
        &fixture.token_program,
        &fixture.ops,
        &metas,
        &fixture.ix_data,
        &[],
    )
    .unwrap();
    let (pda, bump) = Action::find_address(&fixture.vault.address, &hash);
    svm.set_account(
        pda,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
            data: serialize(&RoshiAccount::Action(Action {
                vault: fixture.vault.address.to_bytes(),
                action_hash: hash,
                ops: fixture.ops,
                scope: ActionScope::Swap,
                fee_num: 0,
                fee_den: 0,
                redeem_amount_offset: 0,
                bump,
            }))
            .unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    pda
}

/// Swap instruction with explicit custodies, CPI transfer target, and
/// valuation accounts.
#[allow(clippy::too_many_arguments)]
fn swap_ix_with_valuation(
    fixture: &SwapFixture,
    input_custody: Pubkey,
    output_custody: Pubkey,
    action: Pubkey,
    cpi_from: Pubkey,
    cpi_to: Pubkey,
    valuation: Vec<AccountMeta>,
    ix_data: Vec<u8>,
) -> Instruction {
    roshi_client::instruction::swap(
        fixture.vault.roles.strategist.pubkey(),
        fixture.vault.address,
        fixture.sub_account,
        input_custody,
        output_custody,
        action,
        valuation,
        vec![
            AccountMeta::new(cpi_from, false),
            AccountMeta::new(cpi_to, false),
            AccountMeta::new_readonly(fixture.sub_account, false),
            AccountMeta::new_readonly(fixture.token_program, false),
        ],
        SwapArgs {
            min_out: 0,
            max_in: u64::MAX,
            sub_account: fixture.sub_account_index,
            program_id: fixture.token_program.to_bytes(),
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

#[test]
fn test_swap_value_bound_accepts_symmetric_base_swap() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fixture.install_action(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);
    set_swap_slippage(&mut svm, &fixture.vault, 100);

    // Both endpoints are the base mint: equal value in and out, no oracle
    // accounts needed.
    send_ok(
        &mut svm,
        fixture.ix(fixture.vault.roles.strategist.pubkey(), 0, SWAP_AMOUNT),
        &fixture.vault.roles.strategist,
    );
    assert_eq!(
        token_balance(&svm, &fixture.output_custody),
        OUTPUT_BALANCE + SWAP_AMOUNT
    );
}

#[test]
fn test_swap_value_bound_blocks_value_leak() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    // An authorized route that pays custody value to an outside account: the
    // output custody never receives anything.
    let leak = Pubkey::new_unique();
    crate::helpers::set_token_account(
        &mut svm,
        leak,
        &fixture.vault.base_mint,
        &Pubkey::new_unique(),
        0,
    );
    let action = install_transfer_action(&mut svm, &fixture, fixture.input_custody, leak);
    let leak_ix = |valuation: Vec<AccountMeta>| {
        swap_ix_with_valuation(
            &fixture,
            fixture.input_custody,
            fixture.output_custody,
            action,
            fixture.input_custody,
            leak,
            valuation,
            fixture.ix_data.clone(),
        )
    };

    // Bound off: only the caller-supplied amount bounds apply, and the swap
    // authority chose not to set them. The leak goes through.
    send_ok(&mut svm, leak_ix(vec![]), &fixture.vault.roles.strategist);
    assert_eq!(token_balance(&svm, &leak), SWAP_AMOUNT);

    // Bound on: zero received value against positive spent value rejects.
    set_swap_slippage(&mut svm, &fixture.vault, 100);
    svm.expire_blockhash();
    assert_roshi_error(
        send(&mut svm, leak_ix(vec![]), &fixture.vault.roles.strategist),
        RoshiError::SlippageExceeded,
    );
    assert_eq!(token_balance(&svm, &leak), SWAP_AMOUNT);
}

#[test]
fn test_swap_rejects_unpriceable_endpoint() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);
    set_swap_slippage(&mut svm, &fixture.vault, 100);

    // Output custody holds a mint that is neither the base mint nor a
    // registered Asset: the endpoint cannot be valued, so the swap rejects
    // regardless of amounts.
    let stray_mint = Pubkey::new_unique();
    crate::helpers::set_mint(&mut svm, stray_mint, &Pubkey::new_unique(), 9);
    let stray_output = Pubkey::new_unique();
    crate::helpers::set_token_account(&mut svm, stray_output, &stray_mint, &fixture.sub_account, 0);
    let action = install_transfer_action(&mut svm, &fixture, fixture.input_custody, stray_output);
    let (unregistered_asset_pda, _) =
        roshi::state::asset::Asset::find_address(&fixture.vault.address, &stray_mint);

    assert_roshi_error(
        send(
            &mut svm,
            swap_ix_with_valuation(
                &fixture,
                fixture.input_custody,
                stray_output,
                action,
                fixture.input_custody,
                stray_output,
                vec![AccountMeta::new_readonly(unregistered_asset_pda, false)],
                fixture.ix_data.clone(),
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::UnpriceableSwapLeg,
    );
}

#[test]
fn test_swap_value_bound_prices_asset_input_through_oracle() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);
    set_swap_slippage(&mut svm, &fixture.vault, 100);

    // Registered asset (9 decimals) priced 2.0 base per whole token via Pyth.
    let asset_mint = Pubkey::new_unique();
    crate::helpers::set_mint(&mut svm, asset_mint, &Pubkey::new_unique(), 9);
    let feed_id = [7u8; 32];
    let asset_pda = install_asset(
        &mut svm,
        &fixture.vault,
        asset_mint,
        roshi::oracle::OracleConfig::pyth(roshi::oracle::PythOracleConfig::new(
            feed_id,
            8,
            i64::MAX as u64,
            250,
        )),
        false,
    );
    let pyth = Pubkey::new_unique();
    crate::helpers::set_pyth_price(&mut svm, pyth, feed_id, 200_000_000, -8, 0);

    let asset_input = Pubkey::new_unique();
    crate::helpers::set_token_account(
        &mut svm,
        asset_input,
        &asset_mint,
        &fixture.sub_account,
        1_000_000_000,
    );
    let leak = Pubkey::new_unique();
    crate::helpers::set_token_account(&mut svm, leak, &asset_mint, &Pubkey::new_unique(), 0);

    // Spend 0.5 whole asset tokens (worth 1_000_000 base atoms) for nothing.
    let amount = 500_000_000u64;
    let ix_data = token_transfer_data(amount);
    let metas = token_transfer_metas(asset_input, leak, fixture.sub_account);
    let hash =
        compute_action_hash_from_metas(&fixture.token_program, &fixture.ops, &metas, &ix_data, &[])
            .unwrap();
    let (action, bump) = Action::find_address(&fixture.vault.address, &hash);
    svm.set_account(
        action,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
            data: serialize(&RoshiAccount::Action(Action {
                vault: fixture.vault.address.to_bytes(),
                action_hash: hash,
                ops: fixture.ops,
                scope: ActionScope::Swap,
                fee_num: 0,
                fee_den: 0,
                redeem_amount_offset: 0,
                bump,
            }))
            .unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    assert_roshi_error(
        send(
            &mut svm,
            swap_ix_with_valuation(
                &fixture,
                asset_input,
                fixture.output_custody,
                action,
                asset_input,
                leak,
                vec![
                    AccountMeta::new_readonly(asset_pda, false),
                    AccountMeta::new_readonly(pyth, false),
                ],
                ix_data,
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::SlippageExceeded,
    );
}

#[test]
fn test_swap_value_bound_prices_routed_asset_swap() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    // Vault base oracle: Pyth 1.0 in the shared quote currency.
    let base_feed = [8u8; 32];
    let builder = VaultBuilder::new().base_oracle(roshi::oracle::OracleConfig::pyth(
        roshi::oracle::PythOracleConfig::new(base_feed, 8, i64::MAX as u64, 250),
    ));
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);

    let sub_account_index = 0;
    let sub_account = VaultSubAccount::find_address(&vault.address, sub_account_index).0;

    // Routed asset: ASSET/QUOTE 2.0 over BASE/QUOTE 1.0.
    let asset_mint = Pubkey::new_unique();
    crate::helpers::set_mint(&mut svm, asset_mint, &Pubkey::new_unique(), 9);
    let asset_feed = [9u8; 32];
    let fixture_vault = crate::helpers::TestVault {
        address: vault.address,
        bump: vault.bump,
        tag: vault.tag.clone(),
        base_mint: vault.base_mint,
        share_mint: vault.share_mint,
        treasury: vault.treasury,
        roles: crate::helpers::VaultRoles {
            admin: vault.roles.admin.insecure_clone(),
            strategist: vault.roles.strategist.insecure_clone(),
            nav_authority: vault.roles.nav_authority.insecure_clone(),
            withdrawal_authority: vault.roles.withdrawal_authority.insecure_clone(),
        },
    };
    let asset_pda = install_asset(
        &mut svm,
        &fixture_vault,
        asset_mint,
        roshi::oracle::OracleConfig::pyth(roshi::oracle::PythOracleConfig::new(
            asset_feed,
            8,
            i64::MAX as u64,
            250,
        )),
        true,
    );
    let pyth_asset = Pubkey::new_unique();
    crate::helpers::set_pyth_price(&mut svm, pyth_asset, asset_feed, 200_000_000, -8, 0);
    let pyth_base = Pubkey::new_unique();
    crate::helpers::set_pyth_price(&mut svm, pyth_base, base_feed, 100_000_000, -8, 0);

    // Both endpoints are the routed asset: the route's source must be the named
    // input (#16 — an unnamed sub-account venue would be an unmeasured drain).
    let asset_input = Pubkey::new_unique();
    crate::helpers::set_token_account(
        &mut svm,
        asset_input,
        &asset_mint,
        &sub_account,
        1_000_000_000,
    );
    let asset_output = Pubkey::new_unique();
    crate::helpers::set_token_account(&mut svm, asset_output, &asset_mint, &sub_account, 0);

    set_swap_slippage(&mut svm, &fixture_vault, 100);
    fund(&mut svm, &fixture_vault.roles.strategist);

    let amount = 500_000_000u64;
    let ix_data = token_transfer_data(amount);
    let ops = Ops::new([
        Op::IngestAccount { index: 0 },
        Op::IngestAccount { index: 1 },
    ])
    .unwrap();
    let metas = token_transfer_metas(asset_input, asset_output, sub_account);
    let hash =
        compute_action_hash_from_metas(&TOKEN_PROGRAM_ID, &ops, &metas, &ix_data, &[]).unwrap();
    let (action, bump) = Action::find_address(&vault.address, &hash);
    svm.set_account(
        action,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
            data: serialize(&RoshiAccount::Action(Action {
                vault: vault.address.to_bytes(),
                action_hash: hash,
                ops,
                scope: ActionScope::Swap,
                fee_num: 0,
                fee_den: 0,
                redeem_amount_offset: 0,
                bump,
            }))
            .unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    // Equal asset in and out: the routed valuation prices both legs through the
    // asset oracle and the shared base leg end-to-end, pinning the layout
    // (input asset, output asset, shared base).
    send_ok(
        &mut svm,
        roshi_client::instruction::swap(
            fixture_vault.roles.strategist.pubkey(),
            vault.address,
            sub_account,
            asset_input,
            asset_output,
            action,
            vec![
                AccountMeta::new_readonly(asset_pda, false),
                AccountMeta::new_readonly(pyth_asset, false),
                AccountMeta::new_readonly(asset_pda, false),
                AccountMeta::new_readonly(pyth_asset, false),
                AccountMeta::new_readonly(pyth_base, false),
            ],
            vec![
                AccountMeta::new(asset_input, false),
                AccountMeta::new(asset_output, false),
                AccountMeta::new_readonly(sub_account, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            SwapArgs {
                min_out: 0,
                max_in: u64::MAX,
                sub_account: sub_account_index,
                program_id: TOKEN_PROGRAM_ID.to_bytes(),
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
        .unwrap(),
        &fixture_vault.roles.strategist,
    );
    assert_eq!(token_balance(&svm, &asset_output), amount);
    assert_eq!(token_balance(&svm, &asset_input), 1_000_000_000 - amount);
}

#[test]
fn test_swap_value_bound_shares_one_base_leg_across_routed_endpoints() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    // Vault base oracle: Pyth 1.0 in the shared quote currency.
    let base_feed = [10u8; 32];
    let builder = VaultBuilder::new().base_oracle(roshi::oracle::OracleConfig::pyth(
        roshi::oracle::PythOracleConfig::new(base_feed, 8, i64::MAX as u64, 250),
    ));
    builder.install_mints(&mut svm);
    let vault = builder.install(&mut svm);
    set_swap_slippage(&mut svm, &vault, 100);
    fund(&mut svm, &vault.roles.strategist);

    let sub_account = VaultSubAccount::find_address(&vault.address, 0).0;

    // Two routed assets: the input worth 2.0 quote, the output worth 4.0.
    let mut routed_asset = |feed_id: [u8; 32], price: i64| {
        let mint = Pubkey::new_unique();
        crate::helpers::set_mint(&mut svm, mint, &Pubkey::new_unique(), 9);
        let pda = install_asset(
            &mut svm,
            &vault,
            mint,
            roshi::oracle::OracleConfig::pyth(roshi::oracle::PythOracleConfig::new(
                feed_id,
                8,
                i64::MAX as u64,
                250,
            )),
            true,
        );
        let pyth = Pubkey::new_unique();
        crate::helpers::set_pyth_price(&mut svm, pyth, feed_id, price, -8, 0);
        (mint, pda, pyth)
    };
    let (mint_a, pda_a, pyth_a) = routed_asset([11u8; 32], 200_000_000);
    let (mint_b, pda_b, pyth_b) = routed_asset([12u8; 32], 400_000_000);
    let pyth_base = Pubkey::new_unique();
    crate::helpers::set_pyth_price(&mut svm, pyth_base, base_feed, 100_000_000, -8, 0);

    let input_a = Pubkey::new_unique();
    crate::helpers::set_token_account(&mut svm, input_a, &mint_a, &sub_account, 1_000_000_000);
    let output_b = Pubkey::new_unique();
    crate::helpers::set_token_account(&mut svm, output_b, &mint_b, &sub_account, 0);
    let leak = Pubkey::new_unique();
    crate::helpers::set_token_account(&mut svm, leak, &mint_a, &Pubkey::new_unique(), 0);

    // The CPI pays asset A out and credits nothing: with both endpoints
    // routed, the single shared base account values both sides — there is no
    // second base slot to feed an inconsistent price into.
    let amount = 500_000_000u64;
    let ix_data = token_transfer_data(amount);
    let ops = Ops::new([
        Op::IngestAccount { index: 0 },
        Op::IngestAccount { index: 1 },
    ])
    .unwrap();
    let metas = token_transfer_metas(input_a, leak, sub_account);
    let hash =
        compute_action_hash_from_metas(&TOKEN_PROGRAM_ID, &ops, &metas, &ix_data, &[]).unwrap();
    let (action, bump) = Action::find_address(&vault.address, &hash);
    svm.set_account(
        action,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
            data: serialize(&RoshiAccount::Action(Action {
                vault: vault.address.to_bytes(),
                action_hash: hash,
                ops,
                scope: ActionScope::Swap,
                fee_num: 0,
                fee_den: 0,
                redeem_amount_offset: 0,
                bump,
            }))
            .unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let ix = roshi_client::instruction::swap(
        vault.roles.strategist.pubkey(),
        vault.address,
        sub_account,
        input_a,
        output_b,
        action,
        vec![
            AccountMeta::new_readonly(pda_a, false),
            AccountMeta::new_readonly(pyth_a, false),
            AccountMeta::new_readonly(pda_b, false),
            AccountMeta::new_readonly(pyth_b, false),
            AccountMeta::new_readonly(pyth_base, false),
        ],
        vec![
            AccountMeta::new(input_a, false),
            AccountMeta::new(leak, false),
            AccountMeta::new_readonly(sub_account, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        SwapArgs {
            min_out: 0,
            max_in: u64::MAX,
            sub_account: 0,
            program_id: TOKEN_PROGRAM_ID.to_bytes(),
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

    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.strategist),
        RoshiError::SlippageExceeded,
    );
    assert_eq!(token_balance(&svm, &leak), 0);
}

// --- Custody scope: the bound must cover every sub-account custody it signs
// for, not just the two named endpoints (#16). ---

const SIBLING_BALANCE: u64 = 750_000;

/// A funded clean token account owned by the fixture sub-account.
fn install_sibling_custody(svm: &mut LiteSVM, fixture: &SwapFixture, balance: u64) -> Pubkey {
    let custody = Pubkey::new_unique();
    set_token_account_with_program(
        svm,
        custody,
        &fixture.vault.base_mint,
        &fixture.sub_account,
        balance,
        fixture.token_program,
    );
    custody
}

/// Empty-`Ops` Swap action: the hash commits only the program id, so the route's
/// account list floats — the real-world Jupiter integration shape and the #16
/// threat surface. Returns the action PDA.
fn install_floating_swap_action(svm: &mut LiteSVM, fixture: &SwapFixture) -> Pubkey {
    let ops = Ops::new(std::iter::empty::<Op>()).unwrap();
    let hash = compute_action_hash_from_metas(&fixture.token_program, &ops, &[], &[], &[]).unwrap();
    let (pda, bump) = Action::find_address(&fixture.vault.address, &hash);
    svm.set_account(
        pda,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
            data: serialize(&RoshiAccount::Action(Action {
                vault: fixture.vault.address.to_bytes(),
                action_hash: hash,
                ops,
                scope: ActionScope::Swap,
                fee_num: 0,
                fee_den: 0,
                redeem_amount_offset: 0,
                bump,
            }))
            .unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    pda
}

/// A swap relaying `route_metas` as the CPI (program appended), with no amount
/// or oracle bound in the way (`min_out = 0`, `max_in = MAX`, no valuation). The
/// account flags mirror each meta's writability; the sub-account is promoted to
/// signer by the relay.
fn swap_ix_route(
    fixture: &SwapFixture,
    input_custody: Pubkey,
    output_custody: Pubkey,
    action: Pubkey,
    route_metas: Vec<AccountMeta>,
    ix_data: Vec<u8>,
) -> Instruction {
    let account_flags = route_metas
        .iter()
        .map(|meta| AccountFlags {
            is_signer: false,
            is_writable: meta.is_writable,
        })
        .collect();
    let accounts_len = route_metas.len() as u8;
    let mut cpi_accounts = route_metas;
    cpi_accounts.push(AccountMeta::new_readonly(fixture.token_program, false));

    roshi_client::instruction::swap(
        fixture.vault.roles.strategist.pubkey(),
        fixture.vault.address,
        fixture.sub_account,
        input_custody,
        output_custody,
        action,
        vec![],
        cpi_accounts,
        SwapArgs {
            min_out: 0,
            max_in: u64::MAX,
            sub_account: fixture.sub_account_index,
            program_id: fixture.token_program.to_bytes(),
            accounts_start: 0,
            accounts_len,
            account_flags,
            ix_data,
        },
    )
    .unwrap()
}

fn token_approve_data(amount: u64) -> Vec<u8> {
    let mut data = vec![4];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

#[test]
fn test_swap_rejects_draining_unnamed_sibling_custody() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    // The sub-account owns a third, funded custody. The attacker names the two
    // empty fixture endpoints as input/output (they stay flat) and routes the
    // drain through the sibling to their own account — the #16 PoC.
    let sibling = install_sibling_custody(&mut svm, &fixture, SIBLING_BALANCE);
    let attacker_dest = Pubkey::new_unique();
    set_token_account_with_program(
        &mut svm,
        attacker_dest,
        &fixture.vault.base_mint,
        &Pubkey::new_unique(),
        0,
        fixture.token_program,
    );
    let action = install_transfer_action(&mut svm, &fixture, sibling, attacker_dest);

    assert_roshi_error(
        send(
            &mut svm,
            swap_ix_with_valuation(
                &fixture,
                fixture.input_custody,
                fixture.output_custody,
                action,
                sibling,
                attacker_dest,
                vec![],
                fixture.ix_data.clone(),
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::SwapCustodyMoved,
    );
    // The drain was rolled back with the failed transaction.
    assert_eq!(token_balance(&svm, &sibling), SIBLING_BALANCE);
}

#[test]
fn test_swap_allows_untouched_sibling_in_route() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    let sibling = install_sibling_custody(&mut svm, &fixture, SIBLING_BALANCE);
    let action = install_floating_swap_action(&mut svm, &fixture);

    // An honest input -> output transfer that also hands the sibling to the
    // route (a 4th meta the SPL transfer ignores). The snapshot records the
    // sibling, sees it unchanged, and lets the swap settle.
    let route = vec![
        AccountMeta::new(fixture.input_custody, false),
        AccountMeta::new(fixture.output_custody, false),
        AccountMeta::new_readonly(fixture.sub_account, false),
        AccountMeta::new(sibling, false),
    ];
    send_ok(
        &mut svm,
        swap_ix_route(
            &fixture,
            fixture.input_custody,
            fixture.output_custody,
            action,
            route,
            token_transfer_data(SWAP_AMOUNT),
        ),
        &fixture.vault.roles.strategist,
    );

    assert_eq!(
        token_balance(&svm, &fixture.input_custody),
        INPUT_BALANCE - SWAP_AMOUNT
    );
    assert_eq!(
        token_balance(&svm, &fixture.output_custody),
        OUTPUT_BALANCE + SWAP_AMOUNT
    );
    assert_eq!(token_balance(&svm, &sibling), SIBLING_BALANCE);
}

#[test]
fn test_swap_rejects_draining_sibling_listed_twice() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    // Aggregator routes list the same account more than once. A duplicated
    // sibling must be snapshotted once and still caught when the route drains it.
    let sibling = install_sibling_custody(&mut svm, &fixture, SIBLING_BALANCE);
    let attacker_dest = Pubkey::new_unique();
    set_token_account_with_program(
        &mut svm,
        attacker_dest,
        &fixture.vault.base_mint,
        &Pubkey::new_unique(),
        0,
        fixture.token_program,
    );
    let action = install_floating_swap_action(&mut svm, &fixture);

    let route = vec![
        AccountMeta::new(sibling, false),
        AccountMeta::new(attacker_dest, false),
        AccountMeta::new_readonly(fixture.sub_account, false),
        AccountMeta::new(sibling, false),
    ];
    assert_roshi_error(
        send(
            &mut svm,
            swap_ix_route(
                &fixture,
                fixture.input_custody,
                fixture.output_custody,
                action,
                route,
                token_transfer_data(SWAP_AMOUNT),
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::SwapCustodyMoved,
    );
    assert_eq!(token_balance(&svm, &sibling), SIBLING_BALANCE);
}

#[test]
fn test_swap_rejects_route_that_delegates_sibling() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = SwapFixture::setup(&mut svm);
    fund(&mut svm, &fixture.vault.roles.strategist);

    // The route leaves the sibling balance flat but grants a delegate — a
    // deferred drain. Balance-unchanged alone wouldn't catch it; the post-CPI
    // clean check rejects the lingering delegate.
    let sibling = install_sibling_custody(&mut svm, &fixture, SIBLING_BALANCE);
    let action = install_floating_swap_action(&mut svm, &fixture);

    let route = vec![
        AccountMeta::new(sibling, false),
        AccountMeta::new_readonly(Pubkey::new_unique(), false),
        AccountMeta::new_readonly(fixture.sub_account, false),
    ];
    assert_roshi_error(
        send(
            &mut svm,
            swap_ix_route(
                &fixture,
                fixture.input_custody,
                fixture.output_custody,
                action,
                route,
                token_approve_data(SWAP_AMOUNT),
            ),
            &fixture.vault.roles.strategist,
        ),
        RoshiError::InvalidTokenAccount,
    );
}
