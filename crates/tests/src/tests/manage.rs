use roshi::{
    instructions::ManageArgs,
    state::{
        action::{compute_action_hash, Action, Ops},
        sub_account::VaultSubAccount,
        Account as RoshiAccount,
    },
    ID,
};
use solana_instruction::{error::InstructionError, AccountMeta};
use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
use solana_system_interface::program as system_program;
use wincode::serialize;

use crate::helpers::{
    assert_instruction_error, fund, send, send_ok, setup_program, TestVault, VaultBuilder,
    VaultRoles,
};

const TRANSFER_LAMPORTS: u64 = 1_000_000;

struct SystemTransferManageFixture {
    vault: TestVault,
    sub_account_pda: solana_pubkey::Pubkey,
    scratch: solana_pubkey::Pubkey,
    transfer_data: Vec<u8>,
    ops: Ops,
    action_hash: [u8; 32],
    action_pda: solana_pubkey::Pubkey,
}

impl SystemTransferManageFixture {
    fn install(svm: &mut litesvm::LiteSVM, authority: &Keypair) -> Self {
        let vault = VaultBuilder::new()
            .tag(b"test")
            .roles(VaultRoles::shared(authority))
            .install(svm);
        let vault_pda = vault.address;

        let (sub_account_pda, _) = VaultSubAccount::find_address(&vault_pda, 0);
        install_system_account(svm, sub_account_pda, TRANSFER_LAMPORTS);

        let scratch = solana_pubkey::Pubkey::new_unique();
        install_system_account(svm, scratch, 0);

        let transfer_data = system_transfer_data(TRANSFER_LAMPORTS);
        let ops = Ops::empty();
        let action_hash =
            compute_action_hash(&system_program::ID, &ops, &[], &transfer_data).unwrap();
        let (action_pda, _) = Action::find_address(&vault_pda, &action_hash);

        Self {
            vault,
            sub_account_pda,
            scratch,
            transfer_data,
            ops,
            action_hash,
            action_pda,
        }
    }

    fn install_authorized_action(&self, svm: &mut litesvm::LiteSVM) {
        let (_, action_bump) = Action::find_address(&self.vault.address, &self.action_hash);
        svm.set_account(
            self.action_pda,
            Account {
                lamports: TRANSFER_LAMPORTS,
                data: serialize(&RoshiAccount::Action(Action {
                    vault: self.vault.address.to_bytes(),
                    action_hash: self.action_hash,
                    ops: self.ops,
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

    fn manage_ix(&self, strategist: solana_pubkey::Pubkey) -> solana_instruction::Instruction {
        roshi_client::instruction::manage(
            strategist,
            self.vault.address,
            self.sub_account_pda,
            self.action_pda,
            vec![
                AccountMeta::new(self.sub_account_pda, false),
                AccountMeta::new(self.scratch, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            ManageArgs {
                sub_account: 0,
                program_id: system_program::ID.to_bytes(),
                accounts_start: 0,
                accounts_len: 2,
                ix_data: self.transfer_data.clone(),
            },
        )
        .unwrap()
    }

    fn scratch_lamports(&self, svm: &litesvm::LiteSVM) -> u64 {
        svm.get_account(&self.scratch).unwrap().lamports
    }
}

fn install_system_account(
    svm: &mut litesvm::LiteSVM,
    address: solana_pubkey::Pubkey,
    lamports: u64,
) {
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

fn system_transfer_data(lamports: u64) -> Vec<u8> {
    let mut data = vec![2, 0, 0, 0];
    data.extend_from_slice(&lamports.to_le_bytes());
    data
}

#[test]
fn test_manage_authority_check() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SystemTransferManageFixture::install(&mut svm, &authority);
    fixture.install_authorized_action(&mut svm);

    send_ok(&mut svm, fixture.manage_ix(authority.pubkey()), &authority);
    assert_eq!(fixture.scratch_lamports(&svm), TRANSFER_LAMPORTS);

    let wrong = Keypair::new();
    fund(&mut svm, &wrong);

    assert_instruction_error(
        send(&mut svm, fixture.manage_ix(wrong.pubkey()), &wrong),
        InstructionError::IllegalOwner,
    );
}

/// End-to-end proof that the `Action` allowlist gates `manage`: an unauthorized
/// CPI is rejected, `authorize_action` enables it, and `revoke_action` disables
/// it again. `admin == strategist == authority` so one signer drives the whole
/// lifecycle.
#[test]
fn test_authorized_action_lifecycle_gates_manage() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let fixture = SystemTransferManageFixture::install(&mut svm, &authority);

    // Before authorization the Action PDA does not exist, so manage is rejected.
    assert_instruction_error(
        send(&mut svm, fixture.manage_ix(authority.pubkey()), &authority),
        InstructionError::IllegalOwner,
    );

    // Authorize the action; manage now succeeds and moves the lamports.
    send_ok(
        &mut svm,
        roshi_client::instruction::authorize_action(
            authority.pubkey(),
            fixture.vault.address,
            fixture.action_pda,
            fixture.action_hash,
            fixture.ops,
        )
        .unwrap(),
        &authority,
    );
    svm.expire_blockhash();
    send_ok(&mut svm, fixture.manage_ix(authority.pubkey()), &authority);
    assert_eq!(fixture.scratch_lamports(&svm), TRANSFER_LAMPORTS);

    // Revoke the action; manage is rejected again (the Action PDA is closed).
    send_ok(
        &mut svm,
        roshi_client::instruction::revoke_action(
            authority.pubkey(),
            fixture.vault.address,
            fixture.action_pda,
            fixture.action_hash,
        )
        .unwrap(),
        &authority,
    );
    svm.expire_blockhash();
    assert_instruction_error(
        send(&mut svm, fixture.manage_ix(authority.pubkey()), &authority),
        InstructionError::IllegalOwner,
    );
}
