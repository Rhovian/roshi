use super::fixture::*;
use crate::helpers::*;
use roshi::{
    error::RoshiError,
    state::action::{Action, ActionScope, Ops},
};
use solana_sdk::signer::Signer;

#[test]
fn deploy_scope_is_rejected_by_manage_and_manage_batch() {
    let (mut svm, ..) = setup_program().expect("build SBF");
    let f = Fixture::new(&mut svm);
    let manage = roshi_client::instruction::manage(
        f.vault.roles.strategist.pubkey(),
        f.vault.address,
        f.sub,
        f.action,
        f.cpi_accounts(),
        f.manage_args(),
    )
    .unwrap();
    let batch = roshi_client::instruction::manage_batch(
        f.vault.roles.strategist.pubkey(),
        f.vault.address,
        vec![roshi_client::instruction::ManageBatchActionAccounts {
            sub_account_pda: f.sub,
            action: f.action,
        }],
        f.cpi_accounts(),
        vec![f.manage_args()],
    )
    .unwrap();
    let before = f.snapshot(&svm);
    for ix in [manage, batch] {
        assert_roshi_error(
            send(&mut svm, ix, &f.vault.roles.strategist),
            RoshiError::UnauthorizedAction,
        );
        assert_eq!(f.snapshot(&svm), before);
    }
}

#[test]
fn deploy_authorization_rejects_empty_ops() {
    let (mut svm, ..) = setup_program().expect("build SBF");
    let f = Fixture::new(&mut svm);
    let hash = [42; 32];
    let action = Action::find_address(&f.vault.address, &hash).0;
    let ix = roshi_client::instruction::authorize_action(
        f.vault.roles.admin.pubkey(),
        f.vault.address,
        action,
        hash,
        ActionScope::Deploy,
        Ops::empty(),
        1,
        0,
        0,
    )
    .unwrap();
    assert_roshi_error(
        send(&mut svm, ix, &f.vault.roles.admin),
        RoshiError::EmptyDeployOps,
    );
    assert!(svm.get_account(&action).is_none());
}

#[test]
fn deposit_and_deploy_retains_deposit_guards() {
    for case in 0..5 {
        let (mut svm, ..) = setup_program().expect("build SBF");
        let f = Fixture::new(&mut svm);
        let mut args = f.args(AMOUNT);
        let expected = match case {
            0 => {
                f.vault.update(&mut svm, |v| v.set_deposits_paused(true));
                RoshiError::VaultPaused
            }
            1 => {
                f.vault.update(&mut svm, |v| {
                    v.set_private(true);
                    v.access_merkle_root = [1; 32];
                });
                RoshiError::InvalidAccessProof
            }
            2 => {
                f.vault.update(&mut svm, |v| v.deposit_cap = AMOUNT - 1);
                RoshiError::DepositCapExceeded
            }
            3 => {
                args.min_shares_out = u64::MAX;
                RoshiError::SlippageExceeded
            }
            4 => {
                args = f.args(0);
                RoshiError::ZeroOutput
            }
            _ => unreachable!(),
        };
        f.rejects(&mut svm, f.ix(args), expected);
    }
}

#[test]
fn deposit_and_deploy_requires_configured_subaccount_and_base_custody() {
    let (mut svm, ..) = setup_program().expect("build SBF");
    let f = Fixture::new(&mut svm);
    let mut ix = f.ix(f.args(AMOUNT));
    ix.accounts[8].pubkey =
        roshi::state::sub_account::VaultSubAccount::find_address(&f.vault.address, 0).0;
    assert_instruction_error(
        send(&mut svm, ix, &f.user),
        solana_instruction::error::InstructionError::InvalidSeeds,
    );
    let other_mint = solana_pubkey::Pubkey::new_unique();
    let non_base = set_ata(&mut svm, &f.sub, &other_mint, 0);
    let mut ix = f.ix(f.args(AMOUNT));
    ix.accounts[3].pubkey = non_base;
    f.rejects(&mut svm, ix, RoshiError::InvalidTokenAccount);
}

#[test]
fn deposit_and_deploy_rejects_other_scopes() {
    for scope in [
        ActionScope::Manager,
        ActionScope::Swap,
        ActionScope::AtomicRedeem,
        ActionScope::FlashApprove,
    ] {
        let (mut svm, ..) = setup_program().expect("build SBF");
        let f = Fixture::new(&mut svm);
        let mut account = svm.get_account(&f.action).unwrap();
        let roshi::state::Account::Action(mut action) =
            wincode::deserialize(&account.data).unwrap()
        else {
            panic!("Action");
        };
        action.scope = scope;
        account.data = wincode::serialize(&roshi::state::Account::Action(action)).unwrap();
        svm.set_account(f.action, account).unwrap();
        f.rejects(
            &mut svm,
            f.ix(f.args(AMOUNT)),
            RoshiError::UnauthorizedAction,
        );
    }
}
