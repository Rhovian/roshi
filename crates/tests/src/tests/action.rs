//! `authorize_action` / `revoke_action` manage the `Action` allowlist that
//! gates `manage`: the admin commits an `(action_hash, ops)` pair to a PDA, and
//! `manage` later re-derives the hash from the requested CPI and requires a
//! match. Authorize creates the PDA; revoke closes it.

use roshi::{
    error::RoshiError,
    instructions::{AuthorizeActionArgs, RevokeActionArgs},
    state::{
        action::{Action, ActionScope, Ops, StoredOp},
        Account as RoshiAccount,
    },
    ID,
};
use solana_instruction::{error::InstructionError, AccountMeta};
use solana_sdk::{signature::Keypair, signer::Signer};
use solana_system_interface::program as system_program;
use wincode::deserialize;

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, send_signed, setup_program,
    TestVault, VaultBuilder,
};

const ACTION_HASH: [u8; 32] = [9; 32];

fn authorize_action_ix(
    vault: &TestVault,
) -> (solana_pubkey::Pubkey, solana_instruction::Instruction) {
    let (action_pda, _) = Action::find_address(&vault.address, &ACTION_HASH);
    let ix = roshi_client::instruction::authorize_action(
        vault.roles.admin.pubkey(),
        vault.address,
        action_pda,
        ACTION_HASH,
        ActionScope::Manager,
        Ops::empty(),
        0,
        0,
        0,
    )
    .unwrap();

    (action_pda, ix)
}

fn authorize_test_action(svm: &mut litesvm::LiteSVM, vault: &TestVault) -> solana_pubkey::Pubkey {
    let (action_pda, ix) = authorize_action_ix(vault);
    send_ok(svm, ix, &vault.roles.admin);
    action_pda
}

#[test]
fn test_authorize_action() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let (action_pda, bump) = Action::find_address(&vault.address, &ACTION_HASH);
    assert!(svm.get_account(&action_pda).is_none());

    let ix = roshi_client::instruction::authorize_action(
        vault.roles.admin.pubkey(),
        vault.address,
        action_pda,
        ACTION_HASH,
        ActionScope::Manager,
        Ops::empty(),
        13,
        0,
        0,
    )
    .unwrap();
    send_ok(&mut svm, ix, &vault.roles.admin);

    let account = svm.get_account(&action_pda).unwrap();
    assert_eq!(account.owner, ID);
    assert_eq!(account.data.len(), Action::SPACE);

    let RoshiAccount::Action(action) = deserialize(&account.data).unwrap() else {
        panic!("expected action account");
    };
    assert_eq!(action.vault, vault.address.to_bytes());
    assert_eq!(action.action_hash, ACTION_HASH);
    assert_eq!(action.scope, ActionScope::Manager);
    assert_eq!(action.ops, Ops::empty());
    assert_eq!(action.redeem_amount_offset, 13);
    assert_eq!(action.bump, bump);
}

#[test]
fn test_authorize_action_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let (action_pda, _) = Action::find_address(&vault.address, &ACTION_HASH);
    let ix = roshi_client::instruction::authorize_action(
        outsider.pubkey(),
        vault.address,
        action_pda,
        ACTION_HASH,
        ActionScope::Manager,
        Ops::empty(),
        0,
        0,
        0,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    assert!(svm.get_account(&action_pda).is_none());
}

#[test]
fn test_authorize_action_rejects_duplicate() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let (action_pda, authorize_ix) = authorize_action_ix(&vault);

    send_ok(&mut svm, authorize_ix, &vault.roles.admin);
    assert!(svm.get_account(&action_pda).is_some());

    // A distinct retry must now fail on the uninitialized-account guard.
    svm.expire_blockhash();
    let (_, authorize_ix) = authorize_action_ix(&vault);
    assert_instruction_error(
        send(&mut svm, authorize_ix, &vault.roles.admin),
        InstructionError::AccountAlreadyInitialized,
    );
}

#[test]
fn test_authorize_action_rejects_invalid_ops() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let (action_pda, _) = Action::find_address(&vault.address, &ACTION_HASH);

    // A StoredOp with an unknown kind is not canonically encoded.
    let mut ops = Ops::empty();
    ops.ops[0] = StoredOp {
        kind: 9,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    ops.ops_len = 1;

    let ix = roshi_client::instruction::authorize_action(
        vault.roles.admin.pubkey(),
        vault.address,
        action_pda,
        ACTION_HASH,
        ActionScope::Manager,
        ops,
        0,
        0,
        0,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &vault.roles.admin),
        RoshiError::InvalidOp,
    );

    assert!(svm.get_account(&action_pda).is_none());
}

#[test]
fn test_authorize_action_requires_admin_signature() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let (action_pda, _) = Action::find_address(&vault.address, &ACTION_HASH);

    // Correct admin key, but its account is not marked as a signer.
    let accounts = vec![
        AccountMeta::new(vault.roles.admin.pubkey(), false),
        AccountMeta::new_readonly(vault.address, false),
        AccountMeta::new(action_pda, false),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    let ix = roshi_client::instruction::new(
        accounts,
        &AuthorizeActionArgs {
            action_hash: ACTION_HASH,
            scope: ActionScope::Manager,
            ops: Ops::empty(),
            redeem_amount_offset: 0,
            fee_num: 0,
            fee_den: 0,
        },
    )
    .unwrap();

    let payer = Keypair::new();
    fund(&mut svm, &payer);
    assert_instruction_error(
        send(&mut svm, ix, &payer),
        InstructionError::MissingRequiredSignature,
    );

    assert!(svm.get_account(&action_pda).is_none());
}

#[test]
fn test_authorize_action_rejects_non_writable_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    let (action_pda, _) = Action::find_address(&vault.address, &ACTION_HASH);

    // Admin signs but its account is read-only, so it cannot fund the PDA.
    let accounts = vec![
        AccountMeta::new_readonly(vault.roles.admin.pubkey(), true),
        AccountMeta::new_readonly(vault.address, false),
        AccountMeta::new(action_pda, false),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    let ix = roshi_client::instruction::new(
        accounts,
        &AuthorizeActionArgs {
            action_hash: ACTION_HASH,
            scope: ActionScope::Manager,
            ops: Ops::empty(),
            redeem_amount_offset: 0,
            fee_num: 0,
            fee_den: 0,
        },
    )
    .unwrap();

    let payer = Keypair::new();
    fund(&mut svm, &payer);
    assert_instruction_error(
        send_signed(&mut svm, ix, &payer, &[&vault.roles.admin]),
        InstructionError::InvalidAccountData,
    );

    assert!(svm.get_account(&action_pda).is_none());
}

#[test]
fn test_authorize_action_rejects_mismatched_seeds() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    // Correct admin, but the action account is not the PDA for the hash.
    let wrong_action = solana_pubkey::Pubkey::new_unique();
    let ix = roshi_client::instruction::authorize_action(
        vault.roles.admin.pubkey(),
        vault.address,
        wrong_action,
        ACTION_HASH,
        ActionScope::Manager,
        Ops::empty(),
        0,
        0,
        0,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::InvalidSeeds,
    );
}

#[test]
fn test_revoke_action() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let action_pda = authorize_test_action(&mut svm, &vault);
    assert!(svm.get_account(&action_pda).is_some());

    send_ok(
        &mut svm,
        roshi_client::instruction::revoke_action(
            vault.roles.admin.pubkey(),
            vault.address,
            action_pda,
            ACTION_HASH,
        )
        .unwrap(),
        &vault.roles.admin,
    );

    // Closing drains lamports, so the account is reaped.
    assert!(svm.get_account(&action_pda).is_none());
}

#[test]
fn test_revoke_action_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let action_pda = authorize_test_action(&mut svm, &vault);

    let outsider = Keypair::new();
    fund(&mut svm, &outsider);
    let ix = roshi_client::instruction::revoke_action(
        outsider.pubkey(),
        vault.address,
        action_pda,
        ACTION_HASH,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &outsider),
        InstructionError::IllegalOwner,
    );

    // The authorized action survives the rejected revoke.
    assert!(svm.get_account(&action_pda).is_some());
}

#[test]
fn test_revoke_action_rejects_mismatched_seeds() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let action_pda = authorize_test_action(&mut svm, &vault);

    // Correct admin and hash, but the supplied account is not the PDA.
    let wrong_action = solana_pubkey::Pubkey::new_unique();
    let ix = roshi_client::instruction::revoke_action(
        vault.roles.admin.pubkey(),
        vault.address,
        wrong_action,
        ACTION_HASH,
    )
    .unwrap();
    assert_instruction_error(
        send(&mut svm, ix, &vault.roles.admin),
        InstructionError::InvalidSeeds,
    );
    assert!(svm.get_account(&action_pda).is_some());
}

#[test]
fn test_revoke_action_requires_admin_signature() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let action_pda = authorize_test_action(&mut svm, &vault);

    // Correct admin key, but its account is not marked as a signer.
    let accounts = vec![
        AccountMeta::new(vault.roles.admin.pubkey(), false),
        AccountMeta::new_readonly(vault.address, false),
        AccountMeta::new(action_pda, false),
    ];
    let ix = roshi_client::instruction::new(
        accounts,
        &RevokeActionArgs {
            action_hash: ACTION_HASH,
        },
    )
    .unwrap();

    let payer = Keypair::new();
    fund(&mut svm, &payer);
    assert_instruction_error(
        send(&mut svm, ix, &payer),
        InstructionError::MissingRequiredSignature,
    );

    // The authorized action survives the rejected revoke.
    assert!(svm.get_account(&action_pda).is_some());
}
