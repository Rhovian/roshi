use litesvm::LiteSVM;
use roshi::{
    instructions::InitializeVaultArgs,
    oracle::OracleConfig,
    state::{vault::Vault, Account as RoshiAccount},
    ID,
};
use solana_instruction::Instruction;
use solana_pubkey::Pubkey;
use solana_sdk::{account::Account, signature::Keypair, signer::Signer};
use wincode::{deserialize, serialize};

use super::{token::set_mint, transaction::send_ok};

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

    /// Install valid SPL base and share mints for this vault so
    /// `InitializeVault`'s mint validation passes: the base mint with the
    /// configured decimals and the share mint with 9 decimals owned by the
    /// vault PDA.
    pub fn install_mints(&self, svm: &mut LiteSVM) {
        let vault_pda = self.address().0;
        set_mint(svm, self.base_mint, &vault_pda, self.base_decimals);
        set_mint(svm, self.share_mint, &vault_pda, 9);
    }

    /// Create the vault through the real `InitializeVault` instruction, signed
    /// by the program authority. Panics if the transaction fails.
    pub fn create(self, svm: &mut LiteSVM, authority: &Keypair, config_pda: Pubkey) -> TestVault {
        self.install_mints(svm);
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
