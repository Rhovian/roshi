mod fixture;
mod guards;
use crate::helpers::*;
use fixture::*;
use roshi::error::RoshiError;
use solana_sdk::signer::Signer;

#[test]
fn deposit_and_deploy_matches_plain_deposit_accounting() {
    let (mut svm, ..) = setup_program().expect("build SBF before running integration tests");
    let f = Fixture::new(&mut svm);
    for (assets, supply, locked, pending) in [
        (0, 0, 0, 0),
        (9 * AMOUNT, 3_000_000_000, AMOUNT, 100_000_000),
    ] {
        set_mint(&mut svm, f.vault.share_mint, &f.vault.address, 9);
        let mut mint = svm.get_account(&f.vault.share_mint).unwrap();
        mint.data[36..44].copy_from_slice(&u64::to_le_bytes(supply));
        svm.set_account(f.vault.share_mint, mint).unwrap();
        f.vault.update(&mut svm, |v| {
            v.total_assets = assets;
            v.requested_withdrawal_shares = pending;
            v.locked_profit = locked;
            v.profit_unlock_start_ts = 100;
            v.profit_unlock_end_ts = 200;
        });
        set_clock_timestamp(&mut svm, 101);
        let mut plain = svm.clone();
        let deposit = roshi_client::instruction::deposit(
            f.user.pubkey(),
            f.vault.address,
            f.source,
            f.custody,
            f.shares,
            f.vault.share_mint,
            TOKEN_PROGRAM_ID,
            f.vault.base_mint,
            AMOUNT,
            0,
            vec![],
            vec![],
        )
        .unwrap();
        send_ok(&mut plain, deposit, &f.user);
        svm.expire_blockhash();
        let custody_before = token_balance(&svm, &f.custody);
        let deployed_before = token_balance(&svm, &f.destination);
        send_ok(&mut svm, f.ix(f.args(AMOUNT)), &f.user);
        for key in [f.vault.address, f.vault.share_mint, f.source, f.shares] {
            assert_eq!(
                svm.get_account(&key).unwrap().data,
                plain.get_account(&key).unwrap().data
            );
        }
        assert_eq!(f.vault.load(&svm).total_assets, assets + AMOUNT);
        assert_eq!(token_balance(&svm, &f.custody), custody_before);
        assert_eq!(
            token_balance(&svm, &f.destination),
            deployed_before + AMOUNT
        );
    }
}

#[test]
fn deposit_and_deploy_rolls_back_deposit_when_cpi_fails() {
    let (mut svm, ..) = setup_program().expect("build SBF");
    let f = Fixture::new(&mut svm);
    // The route is authorized, but its destination is frozen in SPL Token.
    let mut destination = svm.get_account(&f.destination).unwrap();
    destination.data[108] = 2;
    svm.set_account(f.destination, destination).unwrap();
    let before = f.snapshot(&svm);
    let failure = send(&mut svm, f.ix(f.args(AMOUNT)), &f.user).unwrap_err();
    // Both deposit CPIs succeeded before the third (relay) invocation failed.
    let token_success = format!("Program {TOKEN_PROGRAM_ID} success");
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|log| **log == token_success)
            .count(),
        2
    );
    assert_instruction_error(
        Err(failure),
        solana_instruction::error::InstructionError::Custom(17),
    );
    assert_eq!(f.snapshot(&svm), before);
}

#[test]
fn deposit_and_deploy_rejects_amount_mismatch_and_short_payload() {
    for amount in [0, AMOUNT - 1, AMOUNT + 1] {
        let (mut svm, ..) = setup_program().expect("build SBF");
        let f = Fixture::new(&mut svm);
        let mut args = f.args(AMOUNT);
        args.ix_data = Fixture::data(amount);
        f.rejects(&mut svm, f.ix(args), RoshiError::DeployAmountMismatch);
    }
    let (mut svm, ..) = setup_program().expect("build SBF");
    let f = Fixture::new(&mut svm);
    let mut args = f.args(AMOUNT);
    args.ix_data.pop();
    f.rejects(
        &mut svm,
        f.ix(args),
        RoshiError::InstructionSliceOutOfBounds,
    );
}

#[test]
fn deposit_and_deploy_rejects_unbound_writable_and_readonly_metas() {
    for omitted in 0..3 {
        let (mut svm, ..) = setup_program().expect("build SBF");
        let mut f = Fixture::new(&mut svm);
        f.authorize(
            &mut svm,
            Some(omitted),
            roshi::state::action::ActionScope::Deploy,
        );
        f.rejects(
            &mut svm,
            f.ix(f.args(AMOUNT)),
            RoshiError::UnboundDeployAccount,
        );
    }
}

#[test]
fn deposit_and_deploy_hash_pins_destination_flags_selector_and_payload_size() {
    let (svm, ..) = setup_program().expect("build SBF");
    for mutation in 0..5 {
        let mut svm = svm.clone();
        let f = Fixture::new(&mut svm);
        let mut args = f.args(AMOUNT);
        if mutation == 1 {
            let mut flags = args.account_flags.iter().unwrap().collect::<Vec<_>>();
            flags[2].is_writable = true;
            args.account_flags =
                roshi_interface::instructions::PackedAccountFlags::from_flags(&flags);
        } else if mutation == 2 {
            args.ix_data[0] = 4;
        } else if mutation == 3 {
            args.ix_data.push(0);
        }
        let mut ix = f.ix(args);
        if mutation == 0 {
            ix.accounts[11].pubkey = f.source;
        }
        if mutation == 1 {
            ix.accounts[12].is_writable = true;
        }
        if mutation == 4 {
            ix.accounts[12].pubkey = f.user.pubkey();
        }
        f.rejects(&mut svm, ix, RoshiError::UnauthorizedAction);
    }
}

#[test]
fn deposit_and_deploy_supports_token_2022_base_and_private_access() {
    let (mut svm, ..) = setup_program().expect("build SBF");
    let f = Fixture::with_token_program(&mut svm, TOKEN_2022_PROGRAM_ID);
    f.vault.update(&mut svm, |v| {
        v.set_private(true);
        v.access_merkle_root = roshi_interface::access::access_merkle_leaf(&f.user.pubkey());
    });
    let mut args = f.args(AMOUNT);
    args.min_shares_out = 1_000_000_000;
    // Offsets are relative to the CPI section, not the fixed deposit prefix.
    args.accounts_start = 1;
    let mut ix = f.ix(args);
    ix.accounts.insert(
        10,
        solana_instruction::AccountMeta::new_readonly(f.vault.address, false),
    );
    send_ok(&mut svm, ix, &f.user);
    assert_eq!(f.vault.load(&svm).total_assets, AMOUNT);
    assert_eq!(token_balance(&svm, &f.custody), AMOUNT);
    assert_eq!(token_balance(&svm, &f.destination), AMOUNT);
    assert_eq!(token_balance(&svm, &f.shares), 1_000_000_000);
}
