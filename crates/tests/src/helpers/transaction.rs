use litesvm::{
    types::{TransactionMetadata, TransactionResult},
    LiteSVM,
};
use roshi::error::RoshiError;
use solana_instruction::{error::InstructionError, Instruction};
use solana_sdk::{signature::Keypair, signer::Signer, transaction::TransactionError};
use solana_transaction::Transaction;

/// Lamports airdropped to a funded test account (10 SOL).
pub const AIRDROP_LAMPORTS: u64 = 10_000_000_000;

/// Airdrop [`AIRDROP_LAMPORTS`] to `account` so it can pay fees and sign. Use to
/// fund a role keypair (or a fresh outsider) before it submits a transaction.
pub fn fund(svm: &mut LiteSVM, account: &Keypair) {
    svm.airdrop(&account.pubkey(), AIRDROP_LAMPORTS).unwrap();
}

/// Sign `ix` with `payer` as the fee payer and sole signer, then submit it.
#[allow(clippy::result_large_err)]
pub fn send(svm: &mut LiteSVM, ix: Instruction, payer: &Keypair) -> TransactionResult {
    send_signed(svm, ix, payer, &[])
}

/// Like [`send`] but adds `extra_signers` alongside the fee payer. Use when a
/// required signer is not the fee payer (e.g. a role authority that does not
/// also pay fees).
#[allow(clippy::result_large_err)]
pub fn send_signed(
    svm: &mut LiteSVM,
    ix: Instruction,
    payer: &Keypair,
    extra_signers: &[&Keypair],
) -> TransactionResult {
    let mut signers = Vec::with_capacity(1 + extra_signers.len());
    signers.push(payer);
    signers.extend_from_slice(extra_signers);

    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &signers, blockhash);
    svm.send_transaction(tx)
}

/// [`send`] a transaction expected to succeed, returning its metadata. Panics
/// with the program logs if it fails, so positive tests get a useful message.
pub fn send_ok(svm: &mut LiteSVM, ix: Instruction, payer: &Keypair) -> TransactionMetadata {
    match send(svm, ix, payer) {
        Ok(meta) => meta,
        Err(failure) => panic!(
            "transaction failed unexpectedly\nlogs:\n{}",
            failure.meta.pretty_logs(),
        ),
    }
}

/// Like [`send`], but the instruction may mark signers whose keypairs this
/// repo does not hold (the program keypair lives in the deployment repo):
/// their signature slots are left as defaults. Only valid because
/// [`super::setup_program`] builds the SVM with sigverify disabled.
#[allow(clippy::result_large_err)]
pub fn send_partially_signed(
    svm: &mut LiteSVM,
    ix: Instruction,
    payer: &Keypair,
) -> TransactionResult {
    let blockhash = svm.latest_blockhash();
    let message =
        solana_sdk::message::Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let mut tx = Transaction::new_unsigned(message);
    tx.partial_sign(&[payer], blockhash);
    svm.send_transaction(tx)
}

/// [`send_partially_signed`] a transaction expected to succeed.
pub fn send_ok_partially_signed(
    svm: &mut LiteSVM,
    ix: Instruction,
    payer: &Keypair,
) -> TransactionMetadata {
    match send_partially_signed(svm, ix, payer) {
        Ok(meta) => meta,
        Err(failure) => panic!(
            "transaction failed unexpectedly\nlogs:\n{}",
            failure.meta.pretty_logs(),
        ),
    }
}

/// [`send_signed`] a transaction expected to succeed.
pub fn send_ok_signed(
    svm: &mut LiteSVM,
    ix: Instruction,
    payer: &Keypair,
    extra_signers: &[&Keypair],
) -> TransactionMetadata {
    match send_signed(svm, ix, payer, extra_signers) {
        Ok(meta) => meta,
        Err(failure) => panic!(
            "transaction failed unexpectedly\nlogs:\n{}",
            failure.meta.pretty_logs(),
        ),
    }
}

/// Assert that a transaction failed at the instruction level with exactly
/// `expected`. On mismatch the program logs are surfaced so the real cause is
/// visible. Use this for negative tests instead of a bare `is_err()` so a test
/// can't pass for the wrong reason.
///
/// Pass builtin errors directly (e.g. [`InstructionError::InvalidSeeds`]) and
/// program errors as `InstructionError::Custom(RoshiError::X as u32)`.
pub fn assert_instruction_error(result: TransactionResult, expected: InstructionError) {
    let failure = result.expect_err("expected the transaction to fail");
    match &failure.err {
        TransactionError::InstructionError(_, actual) => assert_eq!(
            *actual,
            expected,
            "transaction failed with the wrong instruction error\nlogs:\n{}",
            failure.meta.pretty_logs(),
        ),
        other => panic!(
            "expected an InstructionError, got {other:?}\nlogs:\n{}",
            failure.meta.pretty_logs(),
        ),
    }
}

/// Assert that a transaction failed with a specific [`RoshiError`]. Convenience
/// over [`assert_instruction_error`] for the program's own (custom) errors,
/// which are the common case in negative tests.
pub fn assert_roshi_error(result: TransactionResult, expected: RoshiError) {
    assert_instruction_error(result, InstructionError::Custom(expected as u32));
}
