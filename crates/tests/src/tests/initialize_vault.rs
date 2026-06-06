use roshi::{error::RoshiError, state::vault::Vault, ID};
use solana_instruction::error::InstructionError;
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::helpers::{
    assert_instruction_error, assert_roshi_error, fund, send, send_ok_signed, send_signed,
    set_mint, set_token_account, setup_program, VaultBuilder,
};

#[test]
fn test_initialize_vault() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let vault = VaultBuilder::new()
        .tag(b"main")
        .private(true, [7; 32])
        .create(&mut svm, &authority, config_pda);

    let account = svm.get_account(&vault.address).unwrap();
    assert_eq!(account.owner, ID);
    assert_eq!(account.data.len(), Vault::SPACE);

    let state = vault.load(&svm);
    assert_eq!(state.tag_seed().unwrap(), b"main");
    assert_eq!(state.admin, vault.roles.admin.pubkey().to_bytes());
    assert_eq!(state.strategist, vault.roles.strategist.pubkey().to_bytes());
    assert_eq!(
        state.nav_authority,
        vault.roles.nav_authority.pubkey().to_bytes()
    );
    assert_eq!(
        state.withdrawal_authority,
        vault.roles.withdrawal_authority.pubkey().to_bytes()
    );
    assert_eq!(state.base_mint, vault.base_mint.to_bytes());
    assert_eq!(state.share_mint, vault.share_mint.to_bytes());
    assert_eq!(state.fee_collector, vault.fee_collector.to_bytes());
    assert_eq!(state.base_decimals, 6);
    assert_eq!(state.deposit_sub_account, 0);
    assert_eq!(state.withdraw_sub_account, 1);
    assert_eq!(state.performance_fee_bps, 100);
    assert_eq!(state.withdrawal_buffer_bps, 250);
    assert_eq!(state.total_assets, 0);
    assert_eq!(state.private(), Ok(true));
    assert_eq!(state.access_merkle_root, [7; 32]);
}

#[test]
fn test_initialize_vault_rejects_non_program_authority() {
    let Some((mut svm, _authority, config_pda)) = setup_program() else {
        return;
    };

    let imposter = Keypair::new();
    fund(&mut svm, &imposter);

    let builder = VaultBuilder::new();
    let ix = builder.instruction(imposter.pubkey(), config_pda);

    let share_mint = builder.share_mint_signer().unwrap();
    assert_instruction_error(
        send_signed(&mut svm, ix, &imposter, &[share_mint]),
        InstructionError::IllegalOwner,
    );
    assert!(svm.get_account(&builder.address().0).is_none());
}

#[test]
fn test_initialize_vault_rejects_mismatched_seeds() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    // Correct args, but the vault account passed in does not match the PDA
    // derived from [b"vault", tag, base_mint].
    let builder = VaultBuilder::new();
    let wrong_vault = solana_pubkey::Pubkey::new_unique();
    let ix = builder.instruction_with_vault(authority.pubkey(), config_pda, wrong_vault);

    let share_mint = builder.share_mint_signer().unwrap();
    assert_instruction_error(
        send_signed(&mut svm, ix, &authority, &[share_mint]),
        InstructionError::InvalidSeeds,
    );
    assert!(svm.get_account(&builder.address().0).is_none());
    assert!(svm.get_account(&wrong_vault).is_none());
}

#[test]
fn test_initialize_vault_rejects_reinitialization() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    let vault_pda = builder.address().0;

    builder.install_initialize_vault_accounts(&mut svm);
    let share_mint = builder.share_mint_signer().unwrap();
    send_ok_signed(
        &mut svm,
        builder.instruction(authority.pubkey(), config_pda),
        &authority,
        &[share_mint],
    );

    // Advance the blockhash so the retry is a distinct transaction rather
    // than a duplicate; it must now fail on the uninitialized-account guard.
    svm.expire_blockhash();
    assert_instruction_error(
        send_signed(
            &mut svm,
            builder.instruction(authority.pubkey(), config_pda),
            &authority,
            &[share_mint],
        ),
        InstructionError::AccountAlreadyInitialized,
    );

    // The original vault is untouched.
    let account = svm.get_account(&vault_pda).unwrap();
    assert_eq!(account.owner, ID);
    assert_eq!(account.data.len(), Vault::SPACE);
}

#[test]
fn test_initialize_vault_rejects_matching_base_and_share_mints() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let mint = Keypair::new();
    let builder = VaultBuilder::new()
        .base_mint(mint.pubkey())
        .share_mint_keypair(mint.insecure_clone());
    let ix = builder.instruction(authority.pubkey(), config_pda);

    let share_mint = builder.share_mint_signer().unwrap();
    assert_instruction_error(
        send_signed(&mut svm, ix, &authority, &[share_mint]),
        InstructionError::InvalidArgument,
    );
    assert!(svm.get_account(&builder.address().0).is_none());
}

#[test]
fn test_initialize_vault_rejects_invalid_fee_bps() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    // 10_001 bps exceeds 100% and must be rejected by config validation.
    let builder = VaultBuilder::new().fees(10_001, 0);
    let ix = builder.instruction(authority.pubkey(), config_pda);

    let share_mint = builder.share_mint_signer().unwrap();
    assert_roshi_error(
        send_signed(&mut svm, ix, &authority, &[share_mint]),
        RoshiError::InvalidBps,
    );
    assert!(svm.get_account(&builder.address().0).is_none());
}

#[test]
fn test_initialize_vault_rejects_preinitialized_share_mint() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    let builder = VaultBuilder::new().base_mint(base_mint);
    let vault_pda = builder.address().0;

    // The program now owns share mint creation, so a pre-existing share mint is
    // rejected before any state is written.
    set_mint(&mut svm, base_mint, &vault_pda, 6);
    set_mint(&mut svm, builder.share_mint_key(), &vault_pda, 9);

    let ix = builder.instruction(authority.pubkey(), config_pda);
    let share_mint = builder.share_mint_signer().unwrap();
    assert_instruction_error(
        send_signed(&mut svm, ix, &authority, &[share_mint]),
        InstructionError::AccountAlreadyInitialized,
    );
    assert!(svm.get_account(&vault_pda).is_none());
}

#[test]
fn test_initialize_vault_rejects_missing_share_mint_signature() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    let mut ix = builder.instruction(authority.pubkey(), config_pda);
    ix.accounts[5].is_signer = false;
    assert_instruction_error(
        send(&mut svm, ix, &authority),
        InstructionError::MissingRequiredSignature,
    );
    assert!(svm.get_account(&builder.address().0).is_none());
}

#[test]
fn test_initialize_vault_rejects_readonly_share_mint() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let builder = VaultBuilder::new();
    let mut ix = builder.instruction(authority.pubkey(), config_pda);
    ix.accounts[5].is_writable = false;
    let share_mint = builder.share_mint_signer().unwrap();

    assert_instruction_error(
        send_signed(&mut svm, ix, &authority, &[share_mint]),
        InstructionError::InvalidAccountData,
    );
    assert!(svm.get_account(&builder.address().0).is_none());
}

#[test]
fn test_initialize_vault_rejects_fee_collector_for_wrong_mint() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let fee_collector = solana_pubkey::Pubkey::new_unique();
    let builder = VaultBuilder::new().fee_collector(fee_collector);
    let vault_pda = builder.address().0;
    set_mint(&mut svm, builder.base_mint_key(), &vault_pda, 6);
    set_token_account(
        &mut svm,
        fee_collector,
        &solana_pubkey::Pubkey::new_unique(),
        &solana_pubkey::Pubkey::new_unique(),
        0,
    );
    let ix = builder.instruction(authority.pubkey(), config_pda);

    let share_mint = builder.share_mint_signer().unwrap();
    assert_roshi_error(
        send_signed(&mut svm, ix, &authority, &[share_mint]),
        RoshiError::InvalidTokenAccount,
    );
    assert!(svm.get_account(&builder.address().0).is_none());
}

#[test]
fn test_initialize_vault_rejects_wrong_base_mint_decimals() {
    let Some((mut svm, authority, config_pda)) = setup_program() else {
        return;
    };

    let base_mint = solana_pubkey::Pubkey::new_unique();
    // Vault declares base_decimals = 6 (the builder default).
    let builder = VaultBuilder::new().base_mint(base_mint);
    let vault_pda = builder.address().0;

    set_mint(&mut svm, base_mint, &vault_pda, 9); // mismatch vs declared 6

    let ix = builder.instruction(authority.pubkey(), config_pda);
    let share_mint = builder.share_mint_signer().unwrap();
    assert_roshi_error(
        send_signed(&mut svm, ix, &authority, &[share_mint]),
        RoshiError::InvalidMintAccount,
    );
    assert!(svm.get_account(&vault_pda).is_none());
}
