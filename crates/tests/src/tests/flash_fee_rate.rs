//! `admin_set_flash_fee_rate` / `strategist_lower_flash_fee_rate` (#22) update a
//! `FlashApprove` action's committed `(fee_num, fee_den)` in place. Raising the
//! cap is a theft lever, so it is admin-only; lowering is fail-safe, so the
//! strategist may do it (strictly below the current rate). The rate is a stored,
//! non-hashed field, so updates mutate it without changing the action PDA.

use roshi::{
    error::RoshiError,
    state::{
        action::{Action, ActionScope, Ops},
        Account as RoshiAccount,
    },
};
use solana_instruction::error::InstructionError;
use solana_sdk::{signature::Keypair, signer::Signer};
use wincode::deserialize;

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok, setup_program, TestVault,
    VaultBuilder,
};

const ACTION_HASH: [u8; 32] = [7; 32];

/// Authorize a `FlashApprove` action with an initial rate and return its PDA.
fn authorize_flash_action(
    svm: &mut litesvm::LiteSVM,
    vault: &TestVault,
    fee_num: u64,
    fee_den: u64,
) -> solana_pubkey::Pubkey {
    let (action_pda, _) = Action::find_address(&vault.address, &ACTION_HASH);
    let ix = roshi_client::instruction::authorize_action(
        vault.roles.admin.pubkey(),
        vault.address,
        action_pda,
        ACTION_HASH,
        ActionScope::FlashApprove,
        Ops::empty(),
        0,
        fee_num,
        fee_den,
    )
    .unwrap();
    send_ok(svm, ix, &vault.roles.admin);
    action_pda
}

fn action_rate(svm: &litesvm::LiteSVM, action_pda: solana_pubkey::Pubkey) -> (u64, u64) {
    let account = svm.get_account(&action_pda).unwrap();
    let RoshiAccount::Action(action) = deserialize(&account.data).unwrap() else {
        panic!("expected an action account");
    };
    (action.fee_num, action.fee_den)
}

#[test]
fn test_admin_set_flash_fee_rate() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);

    let action_pda = authorize_flash_action(&mut svm, &vault, 1, 10);
    assert_eq!(action_rate(&svm, action_pda), (1, 10));

    // The admin may raise...
    send_ok(
        &mut svm,
        roshi_client::instruction::admin_set_flash_fee_rate(
            vault.roles.admin.pubkey(),
            vault.address,
            action_pda,
            3,
            10,
        )
        .unwrap(),
        &vault.roles.admin,
    );
    assert_eq!(action_rate(&svm, action_pda), (3, 10));

    // ...and lower (unrestricted). The PDA is unchanged by the update.
    svm.expire_blockhash();
    send_ok(
        &mut svm,
        roshi_client::instruction::admin_set_flash_fee_rate(
            vault.roles.admin.pubkey(),
            vault.address,
            action_pda,
            1,
            1_000,
        )
        .unwrap(),
        &vault.roles.admin,
    );
    assert_eq!(action_rate(&svm, action_pda), (1, 1_000));
}

#[test]
fn test_admin_set_flash_fee_rate_rejects_non_admin() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    fund(&mut svm, &vault.roles.strategist);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let action_pda = authorize_flash_action(&mut svm, &vault, 1, 10);

    // The strategist (a vault role, but not admin) and an outsider are rejected;
    // raising the cap is admin-only.
    for caller in [&vault.roles.strategist, &outsider] {
        let ix = roshi_client::instruction::admin_set_flash_fee_rate(
            caller.pubkey(),
            vault.address,
            action_pda,
            9,
            10,
        )
        .unwrap();
        assert_instruction_error(send(&mut svm, ix, caller), InstructionError::IllegalOwner);
    }
    assert_eq!(action_rate(&svm, action_pda), (1, 10));
}

#[test]
fn test_strategist_lower_flash_fee_rate() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    fund(&mut svm, &vault.roles.strategist);

    let action_pda = authorize_flash_action(&mut svm, &vault, 1, 10);

    // 0.01 < 0.1: the strategist may lower.
    send_ok(
        &mut svm,
        roshi_client::instruction::strategist_lower_flash_fee_rate(
            vault.roles.strategist.pubkey(),
            vault.address,
            action_pda,
            1,
            100,
        )
        .unwrap(),
        &vault.roles.strategist,
    );
    assert_eq!(action_rate(&svm, action_pda), (1, 100));
}

#[test]
fn test_strategist_lower_rejects_raise_and_equal() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    fund(&mut svm, &vault.roles.strategist);

    let action_pda = authorize_flash_action(&mut svm, &vault, 1, 10);

    // A higher rate (0.5 > 0.1) is rejected...
    assert_roshi_error(
        send(
            &mut svm,
            roshi_client::instruction::strategist_lower_flash_fee_rate(
                vault.roles.strategist.pubkey(),
                vault.address,
                action_pda,
                5,
                10,
            )
            .unwrap(),
            &vault.roles.strategist,
        ),
        RoshiError::FlashFeeRateNotLower,
    );

    // ...as is an equal rate expressed differently (0.1 == 0.1).
    svm.expire_blockhash();
    assert_roshi_error(
        send(
            &mut svm,
            roshi_client::instruction::strategist_lower_flash_fee_rate(
                vault.roles.strategist.pubkey(),
                vault.address,
                action_pda,
                10,
                100,
            )
            .unwrap(),
            &vault.roles.strategist,
        ),
        RoshiError::FlashFeeRateNotLower,
    );
    assert_eq!(action_rate(&svm, action_pda), (1, 10));
}

#[test]
fn test_strategist_lower_rejects_non_strategist() {
    let Some((mut svm, ..)) = setup_program() else {
        return;
    };
    let vault = VaultBuilder::new().install(&mut svm);
    fund(&mut svm, &vault.roles.admin);
    let outsider = Keypair::new();
    fund(&mut svm, &outsider);

    let action_pda = authorize_flash_action(&mut svm, &vault, 1, 10);

    // The admin is not the strategist; neither is an outsider.
    for caller in [&vault.roles.admin, &outsider] {
        let ix = roshi_client::instruction::strategist_lower_flash_fee_rate(
            caller.pubkey(),
            vault.address,
            action_pda,
            1,
            100,
        )
        .unwrap();
        assert_instruction_error(send(&mut svm, ix, caller), InstructionError::IllegalOwner);
    }
    assert_eq!(action_rate(&svm, action_pda), (1, 10));
}
