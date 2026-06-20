//! `IngestSiblingInstruction`/`IngestSiblingAccount` commit a top-level sibling
//! instruction (program id + selector, and optionally an account pubkey) into
//! the action hash, located by index relative to the executing `manage` call.
//! The relayed action here is a System transfer (sub-account -> scratch); the
//! sibling is a separate top-level System transfer placed before it. These are
//! the structural primitives the Kamino flash-leverage flow (#19) builds on.

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

use crate::helpers::{assert_roshi_error, setup_program, TestVault, VaultBuilder, VaultRoles};

const TRANSFER_LAMPORTS: u64 = 1_000_000;
/// System program transfer instruction tag (`2u32` little-endian) — the
/// selector both the relayed CPI and the sibling carry.
const SYSTEM_TRANSFER_SELECTOR: [u8; 4] = [2, 0, 0, 0];

/// The instructions sysvar id, located by key among the relay's accounts.
fn instructions_sysvar_id() -> Pubkey {
    solana_sdk::sysvar::instructions::ID
}

fn system_transfer_data(lamports: u64) -> Vec<u8> {
    let mut data = SYSTEM_TRANSFER_SELECTOR.to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
    data
}

/// A top-level System transfer used as the sibling instruction. `from` signs;
/// the amount is incidental — only its program id, selector, and accounts are
/// committed.
fn sibling_transfer_ix(from: Pubkey, to: Pubkey, lamports: u64) -> Instruction {
    Instruction {
        program_id: system_program::ID,
        accounts: vec![AccountMeta::new(from, true), AccountMeta::new(to, false)],
        data: system_transfer_data(lamports),
    }
}

struct SiblingFixture {
    vault: TestVault,
    sub_account_pda: Pubkey,
    scratch: Pubkey,
    /// Recipient of the sibling transfer; one of the sibling's accounts.
    sibling_dest: Pubkey,
    /// Strategist, fee payer, and sibling `from` — all the one authority.
    executor: Pubkey,
}

impl SiblingFixture {
    fn install(svm: &mut litesvm::LiteSVM, authority: &Keypair) -> Self {
        let vault = VaultBuilder::new()
            .tag(b"sibling")
            .roles(VaultRoles::shared(authority))
            .install(svm);

        let (sub_account_pda, _) = VaultSubAccount::find_address(&vault.address, 0);
        set_system_account(svm, sub_account_pda, TRANSFER_LAMPORTS);

        let scratch = Pubkey::new_unique();
        set_system_account(svm, scratch, 0);
        // Pre-fund rent-exempt so the incidental 1-lamport sibling transfer
        // does not leave a rent-paying account (which would abort the tx for an
        // unrelated reason).
        let sibling_dest = Pubkey::new_unique();
        set_system_account(svm, sibling_dest, svm.minimum_balance_for_rent_exemption(0));

        Self {
            vault,
            sub_account_pda,
            scratch,
            sibling_dest,
            executor: authority.pubkey(),
        }
    }

    /// The data the relayed CPI carries (transfer the full sub-account balance
    /// to scratch).
    fn relayed_ix_data(&self) -> Vec<u8> {
        system_transfer_data(TRANSFER_LAMPORTS)
    }

    /// The sibling as it actually appears in the transaction (relative index
    /// `-1`, i.e. placed immediately before `manage`).
    fn sibling(&self) -> Instruction {
        sibling_transfer_ix(self.executor, self.sibling_dest, 1)
    }

    /// The real sibling's account keys, for off-chain hash precomputation —
    /// mirrors what the relay reads from the instructions sysvar.
    fn resolved_accounts(&self) -> [Pubkey; 2] {
        [self.executor, self.sibling_dest]
    }

    /// Compute the authorized hash for `ops` against an explicit sibling spec,
    /// then install the Action PDA. Taking the spec separately lets negative
    /// tests authorize a deliberately wrong program/selector.
    fn install_action(
        &self,
        svm: &mut litesvm::LiteSVM,
        ops: Ops,
        sibling_program: Pubkey,
        sibling_data: &[u8],
        sibling_accounts: &[Pubkey],
    ) -> Pubkey {
        let siblings = [ResolvedSibling {
            relative_index: -1,
            program_id: sibling_program,
            data: sibling_data,
            accounts: sibling_accounts,
        }];
        let action_hash =
            compute_action_hash_from_metas(&system_program::ID, &ops, &[], &[], &siblings).unwrap();

        let (action_pda, action_bump) = Action::find_address(&self.vault.address, &action_hash);
        svm.set_account(
            action_pda,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(Action::SPACE),
                data: serialize(&RoshiAccount::Action(Action {
                    vault: self.vault.address.to_bytes(),
                    action_hash,
                    ops,
                    scope: ActionScope::Manager,
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

        action_pda
    }

    /// Build the `manage` instruction relaying the sub-account -> scratch
    /// transfer. `with_sysvar` appends the instructions sysvar to the relay's
    /// accounts (outside the CPI meta range).
    fn manage_ix(&self, action_pda: Pubkey, with_sysvar: bool) -> Instruction {
        let mut cpi_accounts = vec![
            AccountMeta::new(self.sub_account_pda, false),
            AccountMeta::new(self.scratch, false),
            AccountMeta::new_readonly(system_program::ID, false),
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
                program_id: system_program::ID.to_bytes(),
                accounts_start: 0,
                accounts_len: 2,
                account_flags: vec![
                    AccountFlags {
                        is_signer: false,
                        is_writable: true,
                    },
                    AccountFlags {
                        is_signer: false,
                        is_writable: true,
                    },
                ],
                ix_data: self.relayed_ix_data(),
            },
        )
        .unwrap()
    }

    fn scratch_lamports(&self, svm: &litesvm::LiteSVM) -> u64 {
        svm.get_account(&self.scratch)
            .map(|account| account.lamports)
            .unwrap_or(0)
    }

    /// Submit `[sibling, manage]` so the sibling sits at relative index `-1`.
    fn send_with_sibling(
        &self,
        svm: &mut litesvm::LiteSVM,
        authority: &Keypair,
        manage_ix: Instruction,
    ) -> TransactionResult {
        self.send(svm, authority, &[self.sibling(), manage_ix])
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

/// `IngestSiblingInstruction{-1, 0, 4}` commits the System program id + the
/// transfer selector of the sibling that sits right before `manage`.
fn selector_ops() -> Ops {
    Ops::new([Op::IngestSiblingInstruction {
        relative_index: -1,
        offset: 0,
        len: 4,
    }])
    .unwrap()
}

#[test]
fn test_relays_with_matching_sibling() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SiblingFixture::install(&mut svm, &authority);
    let action_pda = fixture.install_action(
        &mut svm,
        selector_ops(),
        system_program::ID,
        &SYSTEM_TRANSFER_SELECTOR,
        &[],
    );

    let manage_ix = fixture.manage_ix(action_pda, true);
    fixture
        .send_with_sibling(&mut svm, &authority, manage_ix)
        .expect("relay with matching sibling should succeed");
    assert_eq!(fixture.scratch_lamports(&svm), TRANSFER_LAMPORTS);
}

#[test]
fn test_commits_sibling_account() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SiblingFixture::install(&mut svm, &authority);
    let ops = Ops::new([
        Op::IngestSiblingInstruction {
            relative_index: -1,
            offset: 0,
            len: 4,
        },
        // Commit the sibling's destination account (index 1 of the transfer).
        Op::IngestSiblingAccount {
            relative_index: -1,
            index: 1,
        },
    ])
    .unwrap();
    let accounts = fixture.resolved_accounts();
    let action_pda = fixture.install_action(
        &mut svm,
        ops,
        system_program::ID,
        &SYSTEM_TRANSFER_SELECTOR,
        &accounts,
    );

    let manage_ix = fixture.manage_ix(action_pda, true);
    fixture
        .send_with_sibling(&mut svm, &authority, manage_ix)
        .expect("relay committing a matching sibling account should succeed");
    assert_eq!(fixture.scratch_lamports(&svm), TRANSFER_LAMPORTS);
}

#[test]
fn test_rejects_absent_sibling() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SiblingFixture::install(&mut svm, &authority);
    let action_pda = fixture.install_action(
        &mut svm,
        selector_ops(),
        system_program::ID,
        &SYSTEM_TRANSFER_SELECTOR,
        &[],
    );

    // No sibling: `manage` is the only (index 0) instruction, so relative `-1`
    // points before the start of the transaction.
    let manage_ix = fixture.manage_ix(action_pda, true);
    assert_roshi_error(
        fixture.send(&mut svm, &authority, &[manage_ix]),
        RoshiError::RequiredSiblingMissing,
    );
}

#[test]
fn test_rejects_wrong_program() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SiblingFixture::install(&mut svm, &authority);
    // Authorize against a program that is not the sibling's actual one.
    let action_pda = fixture.install_action(
        &mut svm,
        selector_ops(),
        Pubkey::new_unique(),
        &SYSTEM_TRANSFER_SELECTOR,
        &[],
    );

    let manage_ix = fixture.manage_ix(action_pda, true);
    assert_roshi_error(
        fixture.send_with_sibling(&mut svm, &authority, manage_ix),
        RoshiError::UnauthorizedAction,
    );
}

#[test]
fn test_rejects_wrong_selector() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SiblingFixture::install(&mut svm, &authority);
    // Authorize against a selector the real sibling does not carry.
    let action_pda = fixture.install_action(
        &mut svm,
        selector_ops(),
        system_program::ID,
        &[9, 9, 9, 9],
        &[],
    );

    let manage_ix = fixture.manage_ix(action_pda, true);
    assert_roshi_error(
        fixture.send_with_sibling(&mut svm, &authority, manage_ix),
        RoshiError::UnauthorizedAction,
    );
}

#[test]
fn test_rejects_missing_instructions_sysvar() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SiblingFixture::install(&mut svm, &authority);
    let action_pda = fixture.install_action(
        &mut svm,
        selector_ops(),
        system_program::ID,
        &SYSTEM_TRANSFER_SELECTOR,
        &[],
    );

    // Sibling present, but the relay is not given the instructions sysvar.
    let manage_ix = fixture.manage_ix(action_pda, false);
    assert_roshi_error(
        fixture.send_with_sibling(&mut svm, &authority, manage_ix),
        RoshiError::MissingInstructionsSysvar,
    );
}
