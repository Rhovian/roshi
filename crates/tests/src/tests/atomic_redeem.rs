//! `atomic_redeem`: public user redemption through a pre-authorized unwind CPI.
//! The test venue CPI is an SPL Token transfer from a subaccount-owned venue
//! token account into vault custody; the wrapper bounds that transfer amount by
//! the user's share entitlement before invoking it.

use litesvm::LiteSVM;
use roshi::{
    error::RoshiError,
    instructions::{AccountFlags, AtomicRedeemArgs},
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Ops},
        sub_account::VaultSubAccount,
        Account as RoshiAccount,
    },
    ID,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
use wincode::serialize;

use crate::helpers::{
    assert_roshi_error, associated_token_address, fund, mint_supply, send, send_ok, set_ata,
    set_token_account, setup_program, token_balance, VaultBuilder,
};

const ONE_BASE: u64 = 1_000_000;
const ONE_BASE_SHARES: u64 = 1_000_000_000;
const REDEEM_SHARES: u64 = ONE_BASE_SHARES / 2;
const REDEEM_AMOUNT: u64 = ONE_BASE / 2;
const TRANSFER_AMOUNT_OFFSET: u16 = 1;

struct AtomicRedeemFixture {
    vault: crate::helpers::TestVault,
    owner: Keypair,
    share_account: Pubkey,
    recipient: Pubkey,
    sub_account_index: u8,
    sub_account: Pubkey,
    custody: Pubkey,
    venue_account: Pubkey,
    action_pda: Pubkey,
    action_hash: [u8; 32],
    ix_data: Vec<u8>,
    ops: Ops,
}

impl AtomicRedeemFixture {
    fn setup(svm: &mut LiteSVM) -> Self {
        let builder = VaultBuilder::new();
        builder.install_mints(svm);
        let vault = builder.install(svm);

        let owner = Keypair::new();
        fund(svm, &owner);
        let source = set_ata(svm, &owner.pubkey(), &vault.base_mint, ONE_BASE);
        let share_account = set_ata(svm, &owner.pubkey(), &vault.share_mint, 0);
        let sub_account_index = 0;
        let sub_account = VaultSubAccount::find_address(&vault.address, sub_account_index).0;
        let custody = associated_token_address(&sub_account, &vault.base_mint);
        set_token_account(svm, custody, &vault.base_mint, &sub_account, 0);

        send_ok(
            svm,
            roshi_client::instruction::deposit(
                owner.pubkey(),
                vault.address,
                source,
                custody,
                share_account,
                vault.share_mint,
                vault.base_mint,
                ONE_BASE,
                0,
                vec![],
                vec![],
            )
            .unwrap(),
            &owner,
        );
        svm.expire_blockhash();

        let recipient = Pubkey::new_unique();
        set_token_account(svm, recipient, &vault.base_mint, &owner.pubkey(), 0);
        let venue_account = Pubkey::new_unique();
        set_token_account(
            svm,
            venue_account,
            &vault.base_mint,
            &sub_account,
            REDEEM_AMOUNT,
        );

        let ix_data = token_transfer_data(REDEEM_AMOUNT);
        let ops = Ops::empty();
        let action_metas = token_transfer_metas(venue_account, custody, sub_account);
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
            owner,
            share_account,
            recipient,
            sub_account_index,
            sub_account,
            custody,
            venue_account,
            action_pda,
            action_hash,
            ix_data,
            ops,
        }
    }

    fn install_action(&self, svm: &mut LiteSVM, amount_offset: u16) {
        let (_, action_bump) = Action::find_address(&self.vault.address, &self.action_hash);
        svm.set_account(
            self.action_pda,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
                data: serialize(&RoshiAccount::Action(Action {
                    vault: self.vault.address.to_bytes(),
                    action_hash: self.action_hash,
                    ops: self.ops,
                    scope: ActionScope::AtomicRedeem,
                    redeem_amount_offset: amount_offset,
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

    fn ix(&self, shares: u64, min_output: u64, ix_data: Vec<u8>) -> Instruction {
        roshi_client::instruction::atomic_redeem(
            self.owner.pubkey(),
            self.vault.address,
            self.share_account,
            self.vault.share_mint,
            self.recipient,
            self.custody,
            self.sub_account,
            self.action_pda,
            vec![
                AccountMeta::new(self.venue_account, false),
                AccountMeta::new(self.custody, false),
                AccountMeta::new_readonly(self.sub_account, false),
                AccountMeta::new_readonly(crate::helpers::TOKEN_PROGRAM_ID, false),
            ],
            AtomicRedeemArgs {
                shares,
                min_output,
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
                ix_data,
            },
        )
        .unwrap()
    }
}

#[test]
fn test_atomic_redeem_happy_path() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    send_ok(
        &mut svm,
        fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT, fixture.ix_data.clone()),
        &fixture.owner,
    );

    assert_eq!(token_balance(&svm, &fixture.recipient), REDEEM_AMOUNT);
    assert_eq!(
        token_balance(&svm, &fixture.share_account),
        ONE_BASE_SHARES - REDEEM_SHARES
    );
    assert_eq!(
        mint_supply(&svm, &fixture.vault.share_mint),
        ONE_BASE_SHARES - REDEEM_SHARES
    );
    assert_eq!(
        fixture.vault.load(&svm).total_assets,
        ONE_BASE - REDEEM_AMOUNT
    );
}

#[test]
fn test_atomic_redeem_rejects_withdrawal_amount_above_entitlement() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES / 2, REDEEM_AMOUNT, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::WithdrawalExceedsEntitlement,
    );
}

#[test]
fn test_atomic_redeem_rejects_realized_output_above_entitlement() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    let transfer_amount = REDEEM_AMOUNT + 1;
    let declared_amount = REDEEM_AMOUNT;
    set_token_account(
        &mut svm,
        fixture.venue_account,
        &fixture.vault.base_mint,
        &fixture.sub_account,
        transfer_amount,
    );

    let mut ix_data = token_transfer_data(transfer_amount);
    ix_data.extend_from_slice(&declared_amount.to_le_bytes());
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &token_transfer_metas(fixture.venue_account, fixture.custody, fixture.sub_account),
        &ix_data,
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, 9);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, 0, ix_data),
            &fixture.owner,
        ),
        RoshiError::WithdrawalExceedsEntitlement,
    );
}

#[test]
fn test_atomic_redeem_rejects_received_below_min_output() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT + 1, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::SlippageExceeded,
    );
}

#[test]
fn test_atomic_redeem_rejects_when_withdrawals_paused() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let fixture = AtomicRedeemFixture::setup(&mut svm);
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);
    fund(&mut svm, &fixture.vault.roles.admin);
    send_ok(
        &mut svm,
        roshi_client::instruction::set_pause_flags(
            fixture.vault.roles.admin.pubkey(),
            fixture.vault.address,
            false,
            true,
            false,
        )
        .unwrap(),
        &fixture.vault.roles.admin,
    );
    svm.expire_blockhash();

    assert_roshi_error(
        send(
            &mut svm,
            fixture.ix(REDEEM_SHARES, REDEEM_AMOUNT, fixture.ix_data.clone()),
            &fixture.owner,
        ),
        RoshiError::VaultPaused,
    );
}

#[test]
fn test_atomic_redeem_rejects_share_account_in_cpi_metas() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    let ix_data = token_transfer_data(REDEEM_AMOUNT);
    let malicious_metas = vec![
        AccountMeta::new(fixture.venue_account, false),
        AccountMeta::new(fixture.custody, false),
        AccountMeta::new_readonly(fixture.sub_account, true),
        AccountMeta::new_readonly(fixture.share_account, false),
    ];
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &malicious_metas,
        &ix_data,
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, TRANSFER_AMOUNT_OFFSET);

    let ix = roshi_client::instruction::atomic_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.vault.share_mint,
        fixture.recipient,
        fixture.custody,
        fixture.sub_account,
        fixture.action_pda,
        vec![
            AccountMeta::new(fixture.venue_account, false),
            AccountMeta::new(fixture.custody, false),
            AccountMeta::new_readonly(fixture.sub_account, false),
            AccountMeta::new_readonly(fixture.share_account, false),
            AccountMeta::new_readonly(crate::helpers::TOKEN_PROGRAM_ID, false),
        ],
        AtomicRedeemArgs {
            shares: REDEEM_SHARES,
            min_output: REDEEM_AMOUNT,
            sub_account: fixture.sub_account_index,
            program_id: crate::helpers::TOKEN_PROGRAM_ID.to_bytes(),
            accounts_start: 0,
            accounts_len: 4,
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
        send(&mut svm, ix, &fixture.owner),
        RoshiError::UnauthorizedAction,
    );
}

#[test]
fn test_atomic_redeem_rejects_post_cpi_custody_owner_hijack() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let mut fixture = AtomicRedeemFixture::setup(&mut svm);
    let mut new_owner = [0u8; 32];
    new_owner[31] = 1;
    let new_owner = Pubkey::from(new_owner);
    let ix_data = set_account_owner_data(new_owner);
    fixture.action_hash = compute_action_hash_from_metas(
        &crate::helpers::TOKEN_PROGRAM_ID,
        &fixture.ops,
        &set_account_owner_metas(fixture.custody, fixture.sub_account),
        &ix_data,
    )
    .unwrap();
    fixture.action_pda = Action::find_address(&fixture.vault.address, &fixture.action_hash).0;
    fixture.install_action(&mut svm, 3);

    let ix = roshi_client::instruction::atomic_redeem(
        fixture.owner.pubkey(),
        fixture.vault.address,
        fixture.share_account,
        fixture.vault.share_mint,
        fixture.recipient,
        fixture.custody,
        fixture.sub_account,
        fixture.action_pda,
        vec![
            AccountMeta::new(fixture.custody, false),
            AccountMeta::new_readonly(fixture.sub_account, false),
            AccountMeta::new_readonly(crate::helpers::TOKEN_PROGRAM_ID, false),
        ],
        AtomicRedeemArgs {
            shares: REDEEM_SHARES,
            min_output: 0,
            sub_account: fixture.sub_account_index,
            program_id: crate::helpers::TOKEN_PROGRAM_ID.to_bytes(),
            accounts_start: 0,
            accounts_len: 2,
            account_flags: vec![
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
        send(&mut svm, ix, &fixture.owner),
        RoshiError::InvalidTokenAccount,
    );
    assert_eq!(
        token_account_owner(&svm, fixture.custody),
        fixture.sub_account
    );
    assert_eq!(token_balance(&svm, &fixture.recipient), 0);
    assert_eq!(token_balance(&svm, &fixture.share_account), ONE_BASE_SHARES);
    assert_eq!(
        mint_supply(&svm, &fixture.vault.share_mint),
        ONE_BASE_SHARES
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

fn set_account_owner_data(owner: Pubkey) -> Vec<u8> {
    let mut data = vec![6, 2, 1];
    data.extend_from_slice(owner.as_ref());
    data
}

fn set_account_owner_metas(account: Pubkey, authority: Pubkey) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(account, false),
        AccountMeta::new_readonly(authority, true),
    ]
}

fn token_account_owner(svm: &LiteSVM, address: Pubkey) -> Pubkey {
    let account = svm.get_account(&address).unwrap();
    Pubkey::try_from(&account.data[32..64]).unwrap()
}
