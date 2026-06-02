use solana_account_info::AccountInfo;
use solana_instruction::AccountMeta;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

use roshi_interface::action::{
    compute_action_hash_from_metas as compute_interface_action_hash_from_metas, ActionHashError,
};
use roshi_interface::error::RoshiError;

pub use roshi_interface::action::{Op, Ops, StoredOp, MAX_ACTION_OPS};

#[derive(SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct Action {
    pub vault: [u8; 32],
    pub action_hash: [u8; 32],
    pub ops: Ops,
    pub bump: u8,
}

impl Action {
    pub const SEED: &'static [u8] = b"action";
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    pub fn find_address(vault: &Pubkey, action_hash: &[u8; 32]) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED, vault.as_ref(), action_hash], &crate::ID)
    }

    pub fn verify_vault(&self, vault: &Pubkey) -> ProgramResult {
        if self.vault != vault.to_bytes() {
            return Err(RoshiError::UnauthorizedAction.into());
        }

        Ok(())
    }

    pub fn verify_address(&self, vault: &Pubkey, action_key: &Pubkey) -> ProgramResult {
        let (expected_action_key, expected_bump) = Self::find_address(vault, &self.action_hash);
        if action_key != &expected_action_key || self.bump != expected_bump {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(())
    }

    pub fn verify_for_vault(&self, vault: &Pubkey, action_key: &Pubkey) -> ProgramResult {
        self.verify_vault(vault)?;
        self.verify_address(vault, action_key)
    }
}

pub fn compute_action_hash(
    program_id: &Pubkey,
    ops: &Ops,
    accounts: &[AccountInfo],
    ix_data: &[u8],
) -> Result<[u8; 32], ProgramError> {
    let account_metas = accounts
        .iter()
        .map(|account| AccountMeta {
            pubkey: *account.key,
            is_signer: account.is_signer,
            is_writable: account.is_writable,
        })
        .collect::<Vec<_>>();

    compute_action_hash_from_metas(program_id, ops, &account_metas, ix_data)
}

pub fn compute_action_hash_from_metas(
    program_id: &Pubkey,
    ops: &Ops,
    accounts: &[AccountMeta],
    ix_data: &[u8],
) -> Result<[u8; 32], ProgramError> {
    compute_interface_action_hash_from_metas(program_id, ops, accounts, ix_data)
        .map_err(action_hash_error_to_program_error)
}

/// Validates that `ops` is structurally well-formed: within the op-count limit
/// and canonically encoded. Index/slice bounds depend on the CPI accounts and
/// instruction data, so those are checked at execution time by manage.
pub fn validate_ops(ops: &Ops) -> ProgramResult {
    for op in ops.iter().map_err(action_hash_error_to_program_error)? {
        op.map_err(action_hash_error_to_program_error)?;
    }

    Ok(())
}

fn action_hash_error_to_program_error(error: ActionHashError) -> ProgramError {
    match error {
        ActionHashError::InvalidOp | ActionHashError::TooManyOps => RoshiError::InvalidOp.into(),
        ActionHashError::InstructionSliceOutOfBounds => {
            RoshiError::InstructionSliceOutOfBounds.into()
        }
        ActionHashError::AccountIndexOutOfBounds => RoshiError::AccountIndexOutOfBounds.into(),
        ActionHashError::InvalidInstructionData => ProgramError::InvalidInstructionData,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ingestion_hashes_meta_flags() {
        let program_id = Pubkey::new_unique();
        let account_key = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let ops = Ops::new([Op::IngestAccount { index: 0 }]).unwrap();
        let ix_data = [];

        let mut lamports = 0;
        let mut data = [];
        let readonly_account = AccountInfo::new(
            &account_key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
        );
        let readonly_hash =
            compute_action_hash(&program_id, &ops, &[readonly_account], &ix_data).unwrap();

        let mut lamports = 0;
        let mut data = [];
        let writable_account = AccountInfo::new(
            &account_key,
            false,
            true,
            &mut lamports,
            &mut data,
            &owner,
            false,
        );
        let writable_hash =
            compute_action_hash(&program_id, &ops, &[writable_account], &ix_data).unwrap();

        let mut lamports = 0;
        let mut data = [];
        let signer_account = AccountInfo::new(
            &account_key,
            true,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
        );
        let signer_hash =
            compute_action_hash(&program_id, &ops, &[signer_account], &ix_data).unwrap();

        assert_ne!(readonly_hash, writable_hash);
        assert_ne!(readonly_hash, signer_hash);
    }

    #[test]
    fn action_layout_is_fixed_size() {
        assert_eq!(std::mem::size_of::<StoredOp>(), 4);
        assert_eq!(std::mem::size_of::<Ops>(), MAX_ACTION_OPS * 4 + 1);
        assert_eq!(std::mem::size_of::<Action>(), 194);
        assert_eq!(Action::SPACE, 195);
    }
}
