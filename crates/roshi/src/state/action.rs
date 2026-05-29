use solana_account_info::AccountInfo;
use solana_instruction::AccountMeta;
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;
use solana_sha256_hasher::hashv;
use wincode::{SchemaRead, SchemaWrite};

use crate::error::RoshiError;

#[derive(SchemaWrite, SchemaRead)]
#[wincode(tag_encoding = "u8")]
pub enum Op {
    #[wincode(tag = 0)]
    Noop,
    #[wincode(tag = 1)]
    IngestInstruction { offset: u16, len: u8 },
    #[wincode(tag = 2)]
    IngestAccount { index: u8 },
    #[wincode(tag = 3)]
    IngestInstructionDataSize,
}

#[derive(SchemaWrite, SchemaRead)]
pub struct Ops {
    pub ops: Vec<Op>,
}

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

    pub fn find_address(vault: &Pubkey, action_hash: &[u8; 32]) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED, vault.as_ref(), action_hash], &crate::ID)
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
    let mut chunks = vec![program_id.to_bytes().to_vec()];

    for op in &ops.ops {
        match op {
            Op::Noop => chunks.push(vec![0]),
            Op::IngestInstruction { offset, len } => {
                let start = usize::from(*offset);
                let length = usize::from(*len);
                let end = start
                    .checked_add(length)
                    .ok_or(RoshiError::InstructionSliceOutOfBounds)?;
                let slice = ix_data
                    .get(start..end)
                    .ok_or(RoshiError::InstructionSliceOutOfBounds)?;

                chunks.push(vec![1]);
                chunks.push(offset.to_le_bytes().to_vec());
                chunks.push(vec![*len]);
                chunks.push(slice.to_vec());
            }
            Op::IngestAccount { index } => {
                let account = accounts
                    .get(usize::from(*index))
                    .ok_or(RoshiError::AccountIndexOutOfBounds)?;

                chunks.push(vec![2]);
                chunks.push(vec![*index]);
                chunks.push(account.pubkey.to_bytes().to_vec());
                chunks.push(vec![u8::from(account.is_signer)]);
                chunks.push(vec![u8::from(account.is_writable)]);
            }
            Op::IngestInstructionDataSize => {
                let data_len = u32::try_from(ix_data.len())
                    .map_err(|_| ProgramError::InvalidInstructionData)?;

                chunks.push(vec![3]);
                chunks.push(data_len.to_le_bytes().to_vec());
            }
        }
    }

    let refs = chunks.iter().map(Vec::as_slice).collect::<Vec<_>>();
    Ok(hashv(&refs).to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ingestion_hashes_meta_flags() {
        let program_id = Pubkey::new_unique();
        let account_key = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let ops = Ops {
            ops: vec![Op::IngestAccount { index: 0 }],
        };
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
}
