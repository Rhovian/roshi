//! `ActionScope::FlashApprove` relays an SPL `approve` that grants a one-shot
//! delegate on a sub-account custody account, bound at relay to
//! `delegated_amount == F + ceil_mul(F, fee_num, fee_den)` — `F` read from the
//! bound `flash_borrow` sibling (#18/#19), the fee from the action's committed
//! opaque rate (#21). A second committed sibling — a Roshi `assert_delegate_cleared`
//! after the (simulated) `flash_repay` — makes an over-high fee fail loudly
//! rather than leave a residual delegate.
//!
//! Test transaction layout, relative to the `manage` call (index 1):
//!   ix0  flash_borrow stand-in (System transfer F -> sub-ATA)          (-1)
//!   ix1  manage(approve sub-ATA, delegate=strategist, F+fee)            (0)
//!   ix2  simulated flash_repay (delegated SPL transfer, drains allowance)(+1)
//!   ix3  assert_delegate_cleared(sub-ATA)                               (+2)

use litesvm::types::TransactionResult;
use roshi::{
    error::RoshiError,
    instructions::{AccountFlags, ManageArgs},
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Op, Ops, ResolvedSibling},
        sub_account::VaultSubAccount,
        Account as RoshiAccount,
    },
    ID,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
use solana_system_interface::program as system_program;
use solana_transaction::Transaction;
use wincode::serialize;

use crate::helpers::{
    assert_roshi_error, set_token_account, setup_program, TestVault, VaultBuilder, VaultRoles,
    TOKEN_PROGRAM_ID,
};

/// The flash-borrowed amount the delegate is bound to.
const FLASH_AMOUNT: u64 = 1_000;
/// System transfer tag (`2u32` LE) — the committed selector of the borrow sibling.
const SYSTEM_TRANSFER_SELECTOR: [u8; 4] = [2, 0, 0, 0];
/// SPL Token `Approve` / `Transfer` discriminators.
const SPL_APPROVE_TAG: u8 = 4;
const SPL_TRANSFER_TAG: u8 = 3;
/// `assert_delegate_cleared` instruction discriminator (tag 31, no args).
const ASSERT_DELEGATE_CLEARED_TAG: u8 = 31;

fn instructions_sysvar_id() -> Pubkey {
    solana_sdk::sysvar::instructions::ID
}

/// `ceil(value * num / den)`, mirroring the program's `ceil_mul`.
fn ceil_fee(value: u64, num: u64, den: u64) -> u64 {
    if num == 0 {
        return 0;
    }
    let product = u128::from(value) * u128::from(num);
    u64::try_from(product.div_ceil(u128::from(den))).unwrap()
}

struct FlashFixture {
    vault: TestVault,
    sub_account_pda: Pubkey,
    sub_ata: Pubkey,
    repay_dest: Pubkey,
    /// Strategist, fee payer, borrow `from`, approve delegate, repay authority.
    executor: Pubkey,
}

impl FlashFixture {
    fn install(svm: &mut litesvm::LiteSVM, authority: &Keypair) -> Self {
        let vault = VaultBuilder::new()
            .tag(b"flash")
            .roles(VaultRoles::shared(authority))
            .install(svm);

        let (sub_account_pda, _) = VaultSubAccount::find_address(&vault.address, 0);

        // Custody token account owned by the sub-account, balance >> F + fee.
        let mint = Pubkey::new_unique();
        let sub_ata = Pubkey::new_unique();
        set_token_account(svm, sub_ata, &mint, &sub_account_pda, 100 * FLASH_AMOUNT);
        // Destination for the simulated flash_repay transfer.
        let repay_dest = Pubkey::new_unique();
        set_token_account(svm, repay_dest, &mint, &authority.pubkey(), 0);

        Self {
            vault,
            sub_account_pda,
            sub_ata,
            repay_dest,
            executor: authority.pubkey(),
        }
    }

    /// The two committed siblings: the `flash_borrow` at -1 (System transfer,
    /// destination `borrow_dest`) and the `assert_delegate_cleared` at +2 (Roshi
    /// program, checking the sub-ATA).
    fn ops() -> Ops {
        Ops::new([
            Op::IngestSiblingInstruction {
                relative_index: -1,
                offset: 0,
                len: 4,
            },
            Op::IngestSiblingAccount {
                relative_index: -1,
                index: 1,
            },
            Op::IngestSiblingInstruction {
                relative_index: 2,
                offset: 0,
                len: 1,
            },
            Op::IngestSiblingAccount {
                relative_index: 2,
                index: 0,
            },
        ])
        .unwrap()
    }

    /// Authorize a `FlashApprove` action committing both siblings and the opaque
    /// fee rate. `borrow_dest` is the flash-borrow destination folded into the
    /// hash (the sub-ATA for an honest action).
    fn install_action(
        &self,
        svm: &mut litesvm::LiteSVM,
        fee_num: u64,
        fee_den: u64,
        borrow_dest: Pubkey,
    ) -> Pubkey {
        let ops = Self::ops();
        let borrow_accounts = [self.executor, borrow_dest];
        let cleared_accounts = [self.sub_ata];
        let siblings = [
            ResolvedSibling {
                relative_index: -1,
                program_id: system_program::ID,
                data: &SYSTEM_TRANSFER_SELECTOR,
                accounts: &borrow_accounts,
            },
            ResolvedSibling {
                relative_index: 2,
                program_id: ID,
                data: &[ASSERT_DELEGATE_CLEARED_TAG],
                accounts: &cleared_accounts,
            },
        ];
        let action_hash =
            compute_action_hash_from_metas(&TOKEN_PROGRAM_ID, &ops, &[], &[], &siblings).unwrap();

        let (action_pda, action_bump) = Action::find_address(&self.vault.address, &action_hash);
        svm.set_account(
            action_pda,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
                data: serialize(&RoshiAccount::Action(Action {
                    vault: self.vault.address.to_bytes(),
                    action_hash,
                    ops,
                    fee_num,
                    fee_den,
                    scope: ActionScope::FlashApprove,
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

        action_pda
    }

    fn flash_borrow_ix(&self, dest: Pubkey, lamports: u64) -> Instruction {
        let mut data = SYSTEM_TRANSFER_SELECTOR.to_vec();
        data.extend_from_slice(&lamports.to_le_bytes());
        Instruction {
            program_id: system_program::ID,
            accounts: vec![
                AccountMeta::new(self.executor, true),
                AccountMeta::new(dest, false),
            ],
            data,
        }
    }

    /// `manage` relaying `approve(sub_ata, delegate=strategist, approve_amount)`.
    fn manage_ix(&self, action_pda: Pubkey, approve_amount: u64, with_sysvar: bool) -> Instruction {
        let mut cpi_accounts = vec![
            AccountMeta::new(self.sub_ata, false),
            AccountMeta::new_readonly(self.executor, false), // delegate
            AccountMeta::new_readonly(self.sub_account_pda, false), // owner (promoted signer)
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false), // cpi program
        ];
        if with_sysvar {
            cpi_accounts.push(AccountMeta::new_readonly(instructions_sysvar_id(), false));
        }

        let mut ix_data = vec![SPL_APPROVE_TAG];
        ix_data.extend_from_slice(&approve_amount.to_le_bytes());

        roshi_client::instruction::manage(
            self.executor,
            self.vault.address,
            self.sub_account_pda,
            action_pda,
            cpi_accounts,
            ManageArgs {
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
        .unwrap()
    }

    /// Simulated top-level `flash_repay`: a delegated SPL transfer that pulls
    /// `amount` from the sub-ATA, consuming the allowance.
    fn repay_ix(&self, amount: u64) -> Instruction {
        let mut data = vec![SPL_TRANSFER_TAG];
        data.extend_from_slice(&amount.to_le_bytes());
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(self.sub_ata, false),
                AccountMeta::new(self.repay_dest, false),
                AccountMeta::new_readonly(self.executor, true),
            ],
            data,
        }
    }

    fn cleared_ix(&self) -> Instruction {
        roshi_client::instruction::assert_delegate_cleared(self.sub_ata).unwrap()
    }

    fn delegated_amount(&self, svm: &litesvm::LiteSVM) -> u64 {
        let data = svm.get_account(&self.sub_ata).unwrap().data;
        u64::from_le_bytes(data[121..129].try_into().unwrap())
    }

    fn send(
        &self,
        svm: &mut litesvm::LiteSVM,
        authority: &Keypair,
        instructions: &[Instruction],
    ) -> TransactionResult {
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            instructions,
            Some(&authority.pubkey()),
            &[authority],
            blockhash,
        );
        svm.send_transaction(tx)
    }
}

#[test]
fn test_flash_approve_settles_fee_bearing_entry() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    let (fee_num, fee_den) = (1, 10);
    let fee = ceil_fee(FLASH_AMOUNT, fee_num, fee_den);
    let allowance = FLASH_AMOUNT + fee;
    let action_pda = fixture.install_action(&mut svm, fee_num, fee_den, fixture.sub_ata);

    // Committed rate == the (simulated) actual fee: the repay drains the
    // allowance to zero and the bound cleared-check passes.
    fixture
        .send(
            &mut svm,
            &authority,
            &[
                fixture.flash_borrow_ix(fixture.sub_ata, FLASH_AMOUNT),
                fixture.manage_ix(action_pda, allowance, true),
                fixture.repay_ix(allowance),
                fixture.cleared_ix(),
            ],
        )
        .expect("exact-rate fee-bearing entry should settle");
    assert_eq!(fixture.delegated_amount(&svm), 0, "delegate must clear");
}

#[test]
fn test_flash_approve_zero_rate_matches_f() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    // fee_num == 0 reproduces #19: allowance == F.
    let action_pda = fixture.install_action(&mut svm, 0, 0, fixture.sub_ata);

    fixture
        .send(
            &mut svm,
            &authority,
            &[
                fixture.flash_borrow_ix(fixture.sub_ata, FLASH_AMOUNT),
                fixture.manage_ix(action_pda, FLASH_AMOUNT, true),
                fixture.repay_ix(FLASH_AMOUNT),
                fixture.cleared_ix(),
            ],
        )
        .expect("fee-free entry should settle with allowance == F");
    assert_eq!(fixture.delegated_amount(&svm), 0);
}

#[test]
fn test_flash_approve_rejects_wrong_allowance() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    let (fee_num, fee_den) = (1, 10);
    let allowance = FLASH_AMOUNT + ceil_fee(FLASH_AMOUNT, fee_num, fee_den);
    let action_pda = fixture.install_action(&mut svm, fee_num, fee_den, fixture.sub_ata);

    // Approve one more than F + fee: the relay's bind check fails at manage time.
    assert_roshi_error(
        fixture.send(
            &mut svm,
            &authority,
            &[
                fixture.flash_borrow_ix(fixture.sub_ata, FLASH_AMOUNT),
                fixture.manage_ix(action_pda, allowance + 1, true),
                fixture.repay_ix(allowance),
                fixture.cleared_ix(),
            ],
        ),
        RoshiError::FlashDelegateUnbounded,
    );
}

#[test]
fn test_flash_approve_over_high_rate_leaves_residual() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    // Committed rate (10%) is above the actual fee the lender charges (5%):
    // the approve binds F + committed_fee and passes the manage-time check, but
    // the repay only consumes F + actual_fee, leaving a residual delegate that
    // the bound assert_delegate_cleared catches.
    let (fee_num, fee_den) = (1, 10);
    let committed_allowance = FLASH_AMOUNT + ceil_fee(FLASH_AMOUNT, fee_num, fee_den);
    let actual_allowance = FLASH_AMOUNT + ceil_fee(FLASH_AMOUNT, 1, 20);
    let action_pda = fixture.install_action(&mut svm, fee_num, fee_den, fixture.sub_ata);

    assert_roshi_error(
        fixture.send(
            &mut svm,
            &authority,
            &[
                fixture.flash_borrow_ix(fixture.sub_ata, FLASH_AMOUNT),
                fixture.manage_ix(action_pda, committed_allowance, true),
                fixture.repay_ix(actual_allowance),
                fixture.cleared_ix(),
            ],
        ),
        RoshiError::DelegateNotCleared,
    );
}

#[test]
fn test_flash_approve_rejects_missing_cleared_sibling() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    let allowance = FLASH_AMOUNT + ceil_fee(FLASH_AMOUNT, 1, 10);
    let action_pda = fixture.install_action(&mut svm, 1, 10, fixture.sub_ata);

    // The action commits a cleared-check at +2, but the tx omits it: the relay
    // can't resolve the sibling and rejects.
    assert_roshi_error(
        fixture.send(
            &mut svm,
            &authority,
            &[
                fixture.flash_borrow_ix(fixture.sub_ata, FLASH_AMOUNT),
                fixture.manage_ix(action_pda, allowance, true),
                fixture.repay_ix(allowance),
            ],
        ),
        RoshiError::RequiredSiblingMissing,
    );
}

#[test]
fn test_flash_approve_rejects_borrow_into_other_account() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    // The committed flash-borrow destination is a throwaway account, not the
    // delegated sub-ATA — the borrowed F never lands where it's delegated.
    let elsewhere = Pubkey::new_unique();
    crate::helpers::set_token_account(
        &mut svm,
        elsewhere,
        &Pubkey::new_unique(),
        &authority.pubkey(),
        0,
    );
    let allowance = FLASH_AMOUNT + ceil_fee(FLASH_AMOUNT, 1, 10);
    let action_pda = fixture.install_action(&mut svm, 1, 10, elsewhere);

    assert_roshi_error(
        fixture.send(
            &mut svm,
            &authority,
            &[
                fixture.flash_borrow_ix(elsewhere, FLASH_AMOUNT),
                fixture.manage_ix(action_pda, allowance, true),
                fixture.repay_ix(allowance),
                fixture.cleared_ix(),
            ],
        ),
        RoshiError::FlashDestinationMismatch,
    );
}

#[test]
fn test_assert_delegate_cleared_rejects_live_delegate() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    // Set a delegate directly on the sub-ATA, then call the bare instruction.
    let mut account = svm.get_account(&fixture.sub_ata).unwrap();
    account.data[72..76].copy_from_slice(&1u32.to_le_bytes());
    account.data[76..108].copy_from_slice(fixture.executor.as_ref());
    account.data[121..129].copy_from_slice(&500u64.to_le_bytes());
    svm.set_account(fixture.sub_ata, account).unwrap();

    assert_roshi_error(
        fixture.send(&mut svm, &authority, &[fixture.cleared_ix()]),
        RoshiError::DelegateNotCleared,
    );
}
