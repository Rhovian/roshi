//! `ActionScope::FlashApprove` relays an SPL `approve` that grants a one-shot
//! delegate on a sub-account custody account, exempt from the standard custody
//! reverify but bound at relay so `delegated_amount == F` — the flash-borrowed
//! amount read from the `flash_borrow` sibling the action commits (#18). The
//! relayed CPI here is a real SPL `approve`; the sibling is a top-level System
//! transfer of `F` lamports (its program + selector are committed, and `F`
//! sits right after the 4-byte selector — the same shape klend's
//! `flash_borrow(liquidity_amount)` has after its discriminator).

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
/// System transfer tag (`2u32` LE) — the committed selector of the sibling.
const SYSTEM_TRANSFER_SELECTOR: [u8; 4] = [2, 0, 0, 0];
/// SPL Token `Approve` discriminator.
const SPL_APPROVE_TAG: u8 = 4;

fn instructions_sysvar_id() -> Pubkey {
    solana_sdk::sysvar::instructions::ID
}

/// A top-level System transfer of `lamports` — stands in for `flash_borrow`:
/// program + 4-byte selector are committed, and the amount `F` is the `u64`
/// right after the selector (offset 4).
fn flash_sibling_ix(from: Pubkey, to: Pubkey, lamports: u64) -> Instruction {
    let mut data = SYSTEM_TRANSFER_SELECTOR.to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
    Instruction {
        program_id: system_program::ID,
        accounts: vec![AccountMeta::new(from, true), AccountMeta::new(to, false)],
        data,
    }
}

fn approve_data(amount: u64) -> Vec<u8> {
    let mut data = vec![SPL_APPROVE_TAG];
    data.extend_from_slice(&amount.to_le_bytes());
    data
}

struct FlashFixture {
    vault: TestVault,
    sub_account_pda: Pubkey,
    sub_ata: Pubkey,
    sibling_dest: Pubkey,
    /// Strategist, fee payer, sibling `from`, and the approve's delegate.
    executor: Pubkey,
}

impl FlashFixture {
    fn install(svm: &mut litesvm::LiteSVM, authority: &Keypair) -> Self {
        let vault = VaultBuilder::new()
            .tag(b"flash")
            .roles(VaultRoles::shared(authority))
            .install(svm);

        let (sub_account_pda, _) = VaultSubAccount::find_address(&vault.address, 0);

        // Custody token account owned by the sub-account, balance > F.
        let mint = Pubkey::new_unique();
        let sub_ata = Pubkey::new_unique();
        set_token_account(svm, sub_ata, &mint, &sub_account_pda, 2 * FLASH_AMOUNT);

        // Rent-exempt so the incidental sibling transfer leaves a valid account.
        let sibling_dest = Pubkey::new_unique();
        set_system_account(svm, sibling_dest, svm.minimum_balance_for_rent_exemption(0));

        Self {
            vault,
            sub_account_pda,
            sub_ata,
            sibling_dest,
            executor: authority.pubkey(),
        }
    }

    /// The committed ops: a `flash_borrow`-shaped sibling at relative index -1
    /// whose 4-byte selector is committed (F sits at offset 4, right after it),
    /// plus its destination account (the transfer's `to`, index 1) — which the
    /// relay ties to the approve's source.
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
        ])
        .unwrap()
    }

    fn sibling(&self, dest: Pubkey, flash_amount: u64) -> Instruction {
        flash_sibling_ix(self.executor, dest, flash_amount)
    }

    /// Authorize a `FlashApprove` action whose hash commits the System-program
    /// sibling, its selector, and its destination account (`committed_dest`).
    /// The amount itself is never committed.
    fn install_action(&self, svm: &mut litesvm::LiteSVM, committed_dest: Pubkey) -> Pubkey {
        let ops = Self::ops();
        let accounts = [self.executor, committed_dest];
        let siblings = [ResolvedSibling {
            relative_index: -1,
            program_id: system_program::ID,
            // Selector bytes are all the data the hash folds; trailing amount
            // is read at relay, not committed.
            data: &SYSTEM_TRANSFER_SELECTOR,
            accounts: &accounts,
        }];
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

    /// `manage` relaying `approve(sub_ata, delegate=strategist, amount)` with the
    /// sub-account as the (promoted) authority. `with_sysvar` appends the
    /// instructions sysvar to the relay accounts.
    fn manage_ix(&self, action_pda: Pubkey, amount: u64, with_sysvar: bool) -> Instruction {
        let mut cpi_accounts = vec![
            AccountMeta::new(self.sub_ata, false),
            AccountMeta::new_readonly(self.executor, false), // delegate
            AccountMeta::new_readonly(self.sub_account_pda, false), // owner (promoted signer)
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false), // cpi program
        ];
        if with_sysvar {
            cpi_accounts.push(AccountMeta::new_readonly(instructions_sysvar_id(), false));
        }

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
                ix_data: approve_data(amount),
            },
        )
        .unwrap()
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

    fn delegate_state(&self, svm: &litesvm::LiteSVM) -> (u32, Pubkey, u64) {
        let data = svm.get_account(&self.sub_ata).unwrap().data;
        let tag = u32::from_le_bytes(data[72..76].try_into().unwrap());
        let delegate = Pubkey::try_from(&data[76..108]).unwrap();
        let delegated_amount = u64::from_le_bytes(data[121..129].try_into().unwrap());
        (tag, delegate, delegated_amount)
    }
}

fn set_system_account(svm: &mut litesvm::LiteSVM, address: Pubkey, lamports: u64) {
    svm.set_account(
        address,
        Account {
            lamports,
            data: vec![],
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

#[test]
fn test_flash_approve_binds_delegate_to_flash_amount() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    let action_pda = fixture.install_action(&mut svm, fixture.sub_ata);

    // approve delegates exactly F, and a F-lamport flash sibling sits at -1.
    let manage_ix = fixture.manage_ix(action_pda, FLASH_AMOUNT, true);
    fixture
        .send(
            &mut svm,
            &authority,
            &[fixture.sibling(fixture.sub_ata, FLASH_AMOUNT), manage_ix],
        )
        .expect("flash approve bound to F should relay");

    let (tag, delegate, delegated_amount) = fixture.delegate_state(&svm);
    assert_eq!(tag, 1, "delegate must be set");
    assert_eq!(
        delegate, fixture.executor,
        "delegate must be the strategist"
    );
    assert_eq!(delegated_amount, FLASH_AMOUNT, "allowance must equal F");
}

#[test]
fn test_flash_approve_rejects_over_grant() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    let action_pda = fixture.install_action(&mut svm, fixture.sub_ata);

    // Sibling borrows F, but the approve over-grants (F + 1) → unbounded.
    let manage_ix = fixture.manage_ix(action_pda, FLASH_AMOUNT + 1, true);
    assert_roshi_error(
        fixture.send(
            &mut svm,
            &authority,
            &[fixture.sibling(fixture.sub_ata, FLASH_AMOUNT), manage_ix],
        ),
        RoshiError::FlashDelegateUnbounded,
    );
}

#[test]
fn test_flash_approve_rejects_absent_sibling() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    let action_pda = fixture.install_action(&mut svm, fixture.sub_ata);

    // No sibling: `manage` is the only instruction, so relative -1 is out of range.
    let manage_ix = fixture.manage_ix(action_pda, FLASH_AMOUNT, true);
    assert_roshi_error(
        fixture.send(&mut svm, &authority, &[manage_ix]),
        RoshiError::RequiredSiblingMissing,
    );
}

#[test]
fn test_flash_approve_rejects_missing_sysvar() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    let action_pda = fixture.install_action(&mut svm, fixture.sub_ata);

    // Sibling present, but the relay is not given the instructions sysvar.
    let manage_ix = fixture.manage_ix(action_pda, FLASH_AMOUNT, false);
    assert_roshi_error(
        fixture.send(
            &mut svm,
            &authority,
            &[fixture.sibling(fixture.sub_ata, FLASH_AMOUNT), manage_ix],
        ),
        RoshiError::MissingInstructionsSysvar,
    );
}

#[test]
fn test_flash_approve_rejects_borrow_into_other_account() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = FlashFixture::install(&mut svm, &authority);
    // The committed flash-borrow destination is a throwaway account, NOT the
    // delegated sub-ATA. The flash funds that account (so the hash matches), but
    // the approve delegates the sub-ATA — so the borrowed F never lands in the
    // account being delegated. This is the standing-delegate drain the tie
    // closes.
    let action_pda = fixture.install_action(&mut svm, fixture.sibling_dest);

    let manage_ix = fixture.manage_ix(action_pda, FLASH_AMOUNT, true);
    assert_roshi_error(
        fixture.send(
            &mut svm,
            &authority,
            &[
                fixture.sibling(fixture.sibling_dest, FLASH_AMOUNT),
                manage_ix,
            ],
        ),
        RoshiError::FlashDestinationMismatch,
    );
}
