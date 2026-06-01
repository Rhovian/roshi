#![allow(dead_code)]

use std::path::{Path, PathBuf};

use litesvm::{
    types::{TransactionMetadata, TransactionResult},
    LiteSVM,
};
use roshi::{
    error::RoshiError,
    instructions::InitializeVaultArgs,
    oracle::OracleConfig,
    state::{program_config::ProgramConfig, vault::Vault, Account as RoshiAccount},
    ID,
};
use solana_instruction::{error::InstructionError, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{
    account::Account, signature::Keypair, signer::Signer, transaction::TransactionError,
};
use solana_transaction::{Address, Transaction};
use wincode::{deserialize, serialize};

pub fn program_so_path() -> PathBuf {
    std::env::var_os("ROSHI_PROGRAM_SO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../target/deploy/roshi.so"))
}

pub fn setup_program() -> Option<(LiteSVM, Keypair, Pubkey)> {
    let program_so = program_so_path();
    if !Path::new(&program_so).exists() {
        eprintln!(
            "Skipping LiteSVM program setup; build the SBF first or set ROSHI_PROGRAM_SO: {}",
            program_so.display()
        );
        return None;
    }

    let mut svm = LiteSVM::new();
    svm.add_program_from_file(ID, program_so).unwrap();

    let authority = Keypair::new();
    fund(&mut svm, &authority);

    let (config_pda, _) = ProgramConfig::find_address();
    let ix = initialize_program_ix(&authority.pubkey(), &config_pda, &authority.pubkey());

    send_ok(&mut svm, ix, &authority);

    Some((svm, authority, config_pda))
}

pub fn initialize_program_ix(
    payer: &Pubkey,
    config_pda: &Pubkey,
    authority: &Pubkey,
) -> Instruction {
    roshi_client::instruction::initialize_program(*payer, *config_pda, *authority).unwrap()
}

/// Lamports airdropped to a funded test account (10 SOL).
pub const AIRDROP_LAMPORTS: u64 = 10_000_000_000;

/// Airdrop [`AIRDROP_LAMPORTS`] to `account` so it can pay fees and sign. Use to
/// fund a role keypair (or a fresh outsider) before it submits a transaction.
pub fn fund(svm: &mut LiteSVM, account: &Keypair) {
    svm.airdrop(&account.pubkey(), AIRDROP_LAMPORTS).unwrap();
}

/// Sign `ix` with `payer` as the fee payer and sole signer, then submit it.
pub fn send(svm: &mut LiteSVM, ix: Instruction, payer: &Keypair) -> TransactionResult {
    send_signed(svm, ix, payer, &[])
}

/// Like [`send`] but adds `extra_signers` alongside the fee payer. Use when a
/// required signer is not the fee payer (e.g. a role authority that does not
/// also pay fees).
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
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&Address::from(payer.pubkey())),
        &signers,
        blockhash,
    );
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

/// The four vault role authorities, held as keypairs so tests can sign as any
/// of them when exercising role-gated instructions.
pub struct VaultRoles {
    pub admin: Keypair,
    pub strategist: Keypair,
    pub nav_authority: Keypair,
    pub withdrawal_authority: Keypair,
}

impl VaultRoles {
    /// Generate four independent role keypairs.
    pub fn generate() -> Self {
        Self {
            admin: Keypair::new(),
            strategist: Keypair::new(),
            nav_authority: Keypair::new(),
            withdrawal_authority: Keypair::new(),
        }
    }

    /// Assign every role to a single keypair. Useful when a test signs for all
    /// roles with one signer (e.g. the program authority).
    pub fn shared(keypair: &Keypair) -> Self {
        Self {
            admin: keypair.insecure_clone(),
            strategist: keypair.insecure_clone(),
            nav_authority: keypair.insecure_clone(),
            withdrawal_authority: keypair.insecure_clone(),
        }
    }
}

impl Default for VaultRoles {
    fn default() -> Self {
        Self::generate()
    }
}

/// Builder for a test vault. Carries sane defaults so tests only override the
/// fields they care about, then either [`VaultBuilder::create`] it through the
/// real `InitializeVault` instruction or [`VaultBuilder::install`] its state
/// directly for tests that target other instructions.
pub struct VaultBuilder {
    tag: Vec<u8>,
    base_mint: Pubkey,
    share_mint: Pubkey,
    base_decimals: u8,
    base_oracle: OracleConfig,
    deposit_sub_account: u8,
    withdraw_sub_account: u8,
    fee_collector: Pubkey,
    performance_fee_bps: u16,
    withdrawal_buffer_bps: u16,
    max_change_bps: u16,
    min_update_interval: i64,
    private: bool,
    access_merkle_root: [u8; 32],
    roles: VaultRoles,
}

impl Default for VaultBuilder {
    fn default() -> Self {
        Self {
            tag: b"main".to_vec(),
            base_mint: Pubkey::new_unique(),
            share_mint: Pubkey::new_unique(),
            base_decimals: 6,
            base_oracle: OracleConfig::default(),
            deposit_sub_account: 0,
            withdraw_sub_account: 1,
            fee_collector: Pubkey::new_unique(),
            performance_fee_bps: 100,
            withdrawal_buffer_bps: 250,
            max_change_bps: 500,
            min_update_interval: 60,
            private: false,
            access_merkle_root: [0; 32],
            roles: VaultRoles::generate(),
        }
    }
}

impl VaultBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tag(mut self, tag: &[u8]) -> Self {
        self.tag = tag.to_vec();
        self
    }

    pub fn base_mint(mut self, base_mint: Pubkey) -> Self {
        self.base_mint = base_mint;
        self
    }

    pub fn share_mint(mut self, share_mint: Pubkey) -> Self {
        self.share_mint = share_mint;
        self
    }

    pub fn base_decimals(mut self, base_decimals: u8) -> Self {
        self.base_decimals = base_decimals;
        self
    }

    pub fn base_oracle(mut self, base_oracle: OracleConfig) -> Self {
        self.base_oracle = base_oracle;
        self
    }

    pub fn sub_accounts(mut self, deposit: u8, withdraw: u8) -> Self {
        self.deposit_sub_account = deposit;
        self.withdraw_sub_account = withdraw;
        self
    }

    pub fn fee_collector(mut self, fee_collector: Pubkey) -> Self {
        self.fee_collector = fee_collector;
        self
    }

    pub fn fees(
        mut self,
        performance_bps: u16,
        withdrawal_buffer_bps: u16,
        max_change_bps: u16,
    ) -> Self {
        self.performance_fee_bps = performance_bps;
        self.withdrawal_buffer_bps = withdrawal_buffer_bps;
        self.max_change_bps = max_change_bps;
        self
    }

    pub fn min_update_interval(mut self, min_update_interval: i64) -> Self {
        self.min_update_interval = min_update_interval;
        self
    }

    /// Mark the vault private and set its access Merkle root.
    pub fn private(mut self, private: bool, access_merkle_root: [u8; 32]) -> Self {
        self.private = private;
        self.access_merkle_root = access_merkle_root;
        self
    }

    pub fn roles(mut self, roles: VaultRoles) -> Self {
        self.roles = roles;
        self
    }

    /// The vault PDA and bump implied by the current tag and base mint.
    pub fn address(&self) -> (Pubkey, u8) {
        Vault::find_address(&self.tag, &self.base_mint).unwrap()
    }

    /// The `InitializeVault` args for the current configuration.
    pub fn args(&self) -> InitializeVaultArgs {
        let mut tag = [0u8; 32];
        tag[..self.tag.len()].copy_from_slice(&self.tag);

        InitializeVaultArgs {
            tag,
            tag_len: self.tag.len() as u8,
            admin: self.roles.admin.pubkey().to_bytes(),
            strategist: self.roles.strategist.pubkey().to_bytes(),
            nav_authority: self.roles.nav_authority.pubkey().to_bytes(),
            withdrawal_authority: self.roles.withdrawal_authority.pubkey().to_bytes(),
            base_mint: self.base_mint.to_bytes(),
            share_mint: self.share_mint.to_bytes(),
            base_decimals: self.base_decimals,
            base_oracle: self.base_oracle,
            deposit_sub_account: self.deposit_sub_account,
            withdraw_sub_account: self.withdraw_sub_account,
            fee_collector: self.fee_collector.to_bytes(),
            performance_fee_bps: self.performance_fee_bps,
            withdrawal_buffer_bps: self.withdrawal_buffer_bps,
            max_change_bps: self.max_change_bps,
            min_update_interval: self.min_update_interval,
            private: self.private,
            access_merkle_root: self.access_merkle_root,
        }
    }

    /// The `InitializeVault` instruction, using `authority` as both the program
    /// authority and the payer, with the vault account set to the derived PDA.
    pub fn instruction(&self, authority: Pubkey, config_pda: Pubkey) -> Instruction {
        self.instruction_with_vault(authority, config_pda, self.address().0)
    }

    /// Like [`VaultBuilder::instruction`] but passes an explicit `vault` account
    /// instead of the derived PDA. Used to exercise the program's seed check by
    /// supplying a mismatched account.
    pub fn instruction_with_vault(
        &self,
        authority: Pubkey,
        config_pda: Pubkey,
        vault: Pubkey,
    ) -> Instruction {
        roshi_client::instruction::initialize_vault(
            authority,
            config_pda,
            authority,
            vault,
            self.args(),
        )
        .unwrap()
    }

    /// Create the vault through the real `InitializeVault` instruction, signed
    /// by the program authority. Panics if the transaction fails.
    pub fn create(self, svm: &mut LiteSVM, authority: &Keypair, config_pda: Pubkey) -> TestVault {
        let ix = self.instruction(authority.pubkey(), config_pda);
        send_ok(svm, ix, authority);

        let (address, bump) = self.address();
        self.into_fixture(address, bump)
    }

    /// Write the vault account state directly, bypassing the instruction. Use
    /// when a test targets another instruction and should not depend on
    /// `InitializeVault` succeeding.
    pub fn install(self, svm: &mut LiteSVM) -> TestVault {
        let (address, bump) = self.address();
        let vault = Vault::new(
            &self.tag,
            self.roles.admin.pubkey().to_bytes(),
            self.roles.strategist.pubkey().to_bytes(),
            self.roles.nav_authority.pubkey().to_bytes(),
            self.roles.withdrawal_authority.pubkey().to_bytes(),
            self.base_mint.to_bytes(),
            self.share_mint.to_bytes(),
            self.base_decimals,
            self.base_oracle,
            self.deposit_sub_account,
            self.withdraw_sub_account,
            self.fee_collector.to_bytes(),
            self.performance_fee_bps,
            self.withdrawal_buffer_bps,
            self.max_change_bps,
            self.min_update_interval,
            self.private,
            self.access_merkle_root,
            bump,
        )
        .unwrap();

        svm.set_account(
            address,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(Vault::SPACE),
                data: serialize(&RoshiAccount::Vault(vault)).unwrap(),
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        self.into_fixture(address, bump)
    }

    fn into_fixture(self, address: Pubkey, bump: u8) -> TestVault {
        TestVault {
            address,
            bump,
            tag: self.tag,
            base_mint: self.base_mint,
            share_mint: self.share_mint,
            fee_collector: self.fee_collector,
            roles: self.roles,
        }
    }
}

/// A created test vault: its address plus the inputs a test needs to keep
/// signing and asserting against it.
pub struct TestVault {
    pub address: Pubkey,
    pub bump: u8,
    pub tag: Vec<u8>,
    pub base_mint: Pubkey,
    pub share_mint: Pubkey,
    pub fee_collector: Pubkey,
    pub roles: VaultRoles,
}

impl TestVault {
    /// Deserialize the on-chain vault state. Panics if the account is missing or
    /// not a vault.
    pub fn load(&self, svm: &LiteSVM) -> Vault {
        let account = svm.get_account(&self.address).unwrap();
        let RoshiAccount::Vault(vault) = deserialize(&account.data).unwrap() else {
            panic!("expected vault account");
        };
        vault
    }
}
