//! `swap`: swap-authority execution through a pre-authorized CPI with realized
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
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, set_token_account,
    setup_program, token_balance, VaultBuilder,
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
}

impl SwapFixture {
    fn setup(svm: &mut LiteSVM) -> Self {
        let builder = VaultBuilder::new();
        builder.install_mints(svm);
        let vault = builder.install(svm);

        let sub_account_index = 0;
        let sub_account = VaultSubAccount::find_address(&vault.address, sub_account_index).0;
        let input_custody = Pubkey::new_unique();
        let output_custody = Pubkey::new_unique();
        set_token_account(
            svm,
            input_custody,
            &vault.base_mint,
            &sub_account,
            INPUT_BALANCE,
        );
        set_token_account(
            svm,
            output_custody,
            &vault.base_mint,
            &sub_account,
            OUTPUT_BALANCE,
        );

        let ix_data = token_transfer_data(SWAP_AMOUNT);
        let ops = Ops::new([
            Op::IngestAccount { index: 0 },
            Op::IngestAccount { index: 1 },
        ])
        .unwrap();
        let action_metas = token_transfer_metas(input_custody, output_custody, sub_account);
        let action_hash = compute_action_hash_from_metas(
            &crate::helpers::TOKEN_PROGRAM_ID,
            &ops,
            &action_metas,
            &ix_data,
        )
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

    fn ix(&self, swap_authority: Pubkey, min_out: u64, max_in: u64) -> Instruction {
        roshi_client::instruction::swap(
            swap_authority,
            self.vault.address,
            self.sub_account,
            self.input_custody,
            self.output_custody,
            self.action_pda,
            vec![
                AccountMeta::new(self.input_custody, false),
                AccountMeta::new(self.output_custody, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(crate::helpers::TOKEN_PROGRAM_ID, false),
            ],
            SwapArgs {
                min_out,
                max_in,
                sub_account: self.sub_account_index,
                program_id: crate::helpers::TOKEN_PROGRAM_ID.to_bytes(),
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
    fund(&mut svm, &fixture.vault.roles.swap_authority);

    send_ok(
        &mut svm,
        fixture.ix(
            fixture.vault.roles.swap_authority.pubkey(),
            SWAP_AMOUNT,
            SWAP_AMOUNT,
        ),
        &fixture.vault.roles.swap_authority,
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
    fund(&mut svm, &fixture.vault.roles.swap_authority);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(
                fixture.vault.roles.swap_authority.pubkey(),
                SWAP_AMOUNT + 1,
                SWAP_AMOUNT,
            ),
            &fixture.vault.roles.swap_authority,
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
    fund(&mut svm, &fixture.vault.roles.swap_authority);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(
                fixture.vault.roles.swap_authority.pubkey(),
                SWAP_AMOUNT,
                SWAP_AMOUNT - 1,
            ),
            &fixture.vault.roles.swap_authority,
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
    fund(&mut svm, &fixture.vault.roles.swap_authority);
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
                fixture.vault.roles.swap_authority.pubkey(),
                SWAP_AMOUNT,
                SWAP_AMOUNT,
            ),
            &fixture.vault.roles.swap_authority,
        ),
        RoshiError::VaultPaused,
    );
}

#[test]
fn test_swap_rejects_non_swap_authority_signer() {
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
    fund(&mut svm, &fixture.vault.roles.swap_authority);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(
                fixture.vault.roles.swap_authority.pubkey(),
                SWAP_AMOUNT,
                SWAP_AMOUNT,
            ),
            &fixture.vault.roles.swap_authority,
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
