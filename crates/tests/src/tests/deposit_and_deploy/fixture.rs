use crate::helpers::*;
use litesvm::LiteSVM;
use roshi::{
    error::RoshiError,
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Op, Ops},
        sub_account::VaultSubAccount,
    },
};
use roshi_interface::instructions::{
    AccountFlags, DepositAndDeployArgs, ManageArgs, PackedAccountFlags,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};

pub(super) const AMOUNT: u64 = 1_000_000;
pub(super) struct Fixture {
    pub vault: TestVault,
    pub user: Keypair,
    pub source: Pubkey,
    pub shares: Pubkey,
    pub custody: Pubkey,
    pub sub: Pubkey,
    pub destination: Pubkey,
    pub action: Pubkey,
    pub token_program: Pubkey,
}

impl Fixture {
    pub fn new(svm: &mut LiteSVM) -> Self {
        Self::with_token_program(svm, TOKEN_PROGRAM_ID)
    }

    pub fn with_token_program(svm: &mut LiteSVM, token_program: Pubkey) -> Self {
        let vault = VaultBuilder::new().sub_accounts(7, 1).install(svm);
        set_mint(svm, vault.share_mint, &vault.address, 9);
        let sub = VaultSubAccount::find_address(&vault.address, 7).0;
        let custody = set_ata_with_program(svm, &sub, &vault.base_mint, AMOUNT, token_program);
        let user = Keypair::new();
        fund(svm, &user);
        fund(svm, &vault.roles.admin);
        fund(svm, &vault.roles.strategist);
        let source = set_ata_with_program(
            svm,
            &user.pubkey(),
            &vault.base_mint,
            10 * AMOUNT,
            token_program,
        );
        let shares = set_ata(svm, &user.pubkey(), &vault.share_mint, 0);
        let destination = Pubkey::new_unique();
        set_token_account_with_program(svm, destination, &vault.base_mint, &sub, 0, token_program);
        let mut fixture = Self {
            vault,
            user,
            source,
            shares,
            custody,
            sub,
            destination,
            action: Pubkey::default(),
            token_program,
        };
        fixture.authorize(svm, None, ActionScope::Deploy);
        fixture
    }

    pub fn metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.custody, false),
            AccountMeta::new(self.destination, false),
            AccountMeta::new_readonly(self.sub, true),
        ]
    }

    pub fn cpi_accounts(&self) -> Vec<AccountMeta> {
        let mut metas = self.metas();
        metas[2].is_signer = false;
        metas.push(AccountMeta::new_readonly(self.token_program, false));
        metas
    }

    pub fn authorize(&mut self, svm: &mut LiteSVM, omitted: Option<u8>, scope: ActionScope) {
        let ops = Ops::new(
            (0..3)
                .filter(|i| Some(*i) != omitted)
                .map(|index| Op::IngestAccount { index })
                .chain([
                    Op::IngestInstruction { offset: 0, len: 1 },
                    Op::IngestInstructionDataSize,
                ]),
        )
        .unwrap();
        let hash = compute_action_hash_from_metas(
            &self.token_program,
            &ops,
            &self.metas(),
            &Self::data(AMOUNT),
            &[],
        )
        .unwrap();
        self.action = Action::find_address(&self.vault.address, &hash).0;
        let ix = roshi_client::instruction::authorize_action(
            self.vault.roles.admin.pubkey(),
            self.vault.address,
            self.action,
            hash,
            scope,
            ops,
            1,
            0,
            0,
        )
        .unwrap();
        send_ok(svm, ix, &self.vault.roles.admin);
    }

    pub fn data(amount: u64) -> Vec<u8> {
        let mut data = vec![3];
        data.extend(amount.to_le_bytes());
        data
    }

    pub fn args(&self, amount: u64) -> DepositAndDeployArgs {
        DepositAndDeployArgs {
            amount,
            min_shares_out: 0,
            access_proof: vec![],
            accounts_start: 0,
            account_flags: PackedAccountFlags::from_flags(&[
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: true,
                },
                AccountFlags {
                    is_signer: false,
                    is_writable: false,
                },
            ]),
            ix_data: Self::data(amount),
        }
    }

    pub fn ix(&self, args: DepositAndDeployArgs) -> Instruction {
        roshi_client::instruction::deposit_and_deploy(
            self.user.pubkey(),
            self.vault.address,
            self.source,
            self.custody,
            self.shares,
            self.vault.share_mint,
            self.token_program,
            self.sub,
            self.action,
            self.cpi_accounts(),
            args,
        )
        .unwrap()
    }

    pub fn manage_args(&self) -> ManageArgs {
        let args = self.args(AMOUNT);
        ManageArgs {
            sub_account: 7,
            accounts_start: 0,
            account_flags: args.account_flags,
            ix_data: args.ix_data,
        }
    }

    pub fn snapshot(&self, svm: &LiteSVM) -> Vec<Vec<u8>> {
        [
            self.vault.address,
            self.vault.share_mint,
            self.source,
            self.shares,
            self.custody,
            self.destination,
        ]
        .iter()
        .map(|key| svm.get_account(key).unwrap().data)
        .collect()
    }

    pub fn rejects(&self, svm: &mut LiteSVM, ix: Instruction, error: RoshiError) {
        let before = self.snapshot(svm);
        assert_roshi_error(send(svm, ix, &self.user), error);
        assert_eq!(self.snapshot(svm), before);
    }
}
