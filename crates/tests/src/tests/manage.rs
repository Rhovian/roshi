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
use solana_transaction::{Address, Transaction};
use wincode::serialize;

use crate::helpers::{
    assert_instruction_error, send, send_ok, setup_program, VaultBuilder, VaultRoles,
};

#[test]
fn test_manage_authority_check() {
    let Some((mut svm, authority, _config_pda)) = setup_program() else {
        return;
    };

    let scratch = solana_pubkey::Pubkey::new_unique();
    svm.set_account(
        scratch,
        Account {
            lamports: 0,
            data: vec![],
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let mut transfer_data = vec![2, 0, 0, 0];
    transfer_data.extend_from_slice(&1_000_000u64.to_le_bytes());

    let vault = VaultBuilder::new()
        .tag(b"test")
        .roles(VaultRoles::shared(&authority))
        .install(&mut svm);
    let vault_pda = vault.address;

    let (sub_account_pda, _) = VaultSubAccount::find_address(&vault_pda, 0);
    svm.set_account(
        sub_account_pda,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let ops = Ops::empty();
    let action_hash = compute_action_hash(&system_program::ID, &ops, &[], &transfer_data).unwrap();
    let (action_pda, action_bump) = Action::find_address(&vault_pda, &action_hash);
    svm.set_account(
        action_pda,
        Account {
            lamports: 1_000_000,
            data: serialize(&RoshiAccount::Action(Action {
                vault: vault_pda.to_bytes(),
                action_hash,
                ops,
                bump: action_bump,
            }))
            .unwrap(),
            owner: ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let manage_args = ManageArgs {
        sub_account: 0,
        program_id: system_program::ID.to_bytes(),
        accounts_start: 0,
        accounts_len: 2,
        ix_data: transfer_data.clone(),
    };

    let ix = roshi_client::instruction::manage(
        authority.pubkey(),
        vault_pda,
        sub_account_pda,
        action_pda,
        vec![
            AccountMeta::new(sub_account_pda, false),
            AccountMeta::new(scratch, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        manage_args,
    )
    .unwrap();

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&Address::from(authority.pubkey())),
        &[&authority],
        blockhash,
    );
    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "manage transfer failed: {result:?}");
    assert_eq!(svm.get_account(&scratch).unwrap().lamports, 1_000_000);

    let wrong = Keypair::new();
    svm.airdrop(&wrong.pubkey(), 10_000_000_000).unwrap();

    let ix = roshi_client::instruction::manage(
        wrong.pubkey(),
        vault_pda,
        sub_account_pda,
        action_pda,
        vec![
            AccountMeta::new(sub_account_pda, false),
            AccountMeta::new(scratch, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        ManageArgs {
            sub_account: 0,
            program_id: system_program::ID.to_bytes(),
            accounts_start: 0,
            accounts_len: 2,
            ix_data: transfer_data,
        },
    )
    .unwrap();

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&Address::from(wrong.pubkey())),
        &[&wrong],
        blockhash,
    );
    assert!(svm.send_transaction(tx).is_err());
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

    let vault = VaultBuilder::new()
        .tag(b"test")
        .roles(VaultRoles::shared(&authority))
        .install(&mut svm);
    let vault_pda = vault.address;

    // Fund the sub-account so the authorized CPI (a system transfer) can run.
    let (sub_account_pda, _) = VaultSubAccount::find_address(&vault_pda, 0);
    svm.set_account(
        sub_account_pda,
        Account {
            lamports: 1_000_000,
            data: vec![],
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let scratch = solana_pubkey::Pubkey::new_unique();
    svm.set_account(
        scratch,
        Account {
            lamports: 0,
            data: vec![],
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let mut transfer_data = vec![2, 0, 0, 0];
    transfer_data.extend_from_slice(&1_000_000u64.to_le_bytes());

    let ops = Ops::empty();
    let action_hash = compute_action_hash(&system_program::ID, &ops, &[], &transfer_data).unwrap();
    let (action_pda, _) = Action::find_address(&vault_pda, &action_hash);

    let manage_ix = || {
        roshi_client::instruction::manage(
            authority.pubkey(),
            vault_pda,
            sub_account_pda,
            action_pda,
            vec![
                AccountMeta::new(sub_account_pda, false),
                AccountMeta::new(scratch, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            ManageArgs {
                sub_account: 0,
                program_id: system_program::ID.to_bytes(),
                accounts_start: 0,
                accounts_len: 2,
                ix_data: transfer_data.clone(),
            },
        )
        .unwrap()
    };

    // Before authorization the Action PDA does not exist, so manage is rejected.
    assert_instruction_error(
        send(&mut svm, manage_ix(), &authority),
        InstructionError::IllegalOwner,
    );

    // Authorize the action; manage now succeeds and moves the lamports.
    send_ok(
        &mut svm,
        roshi_client::instruction::authorize_action(
            authority.pubkey(),
            vault_pda,
            action_pda,
            action_hash,
            ops,
        )
        .unwrap(),
        &authority,
    );
    svm.expire_blockhash();
    send_ok(&mut svm, manage_ix(), &authority);
    assert_eq!(svm.get_account(&scratch).unwrap().lamports, 1_000_000);

    // Revoke the action; manage is rejected again (the Action PDA is closed).
    send_ok(
        &mut svm,
        roshi_client::instruction::revoke_action(
            authority.pubkey(),
            vault_pda,
            action_pda,
            action_hash,
        )
        .unwrap(),
        &authority,
    );
    svm.expire_blockhash();
    assert_instruction_error(
        send(&mut svm, manage_ix(), &authority),
        InstructionError::IllegalOwner,
    );
}
