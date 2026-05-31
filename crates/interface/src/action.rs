use solana_instruction::AccountMeta;
use solana_pubkey::Pubkey;
use solana_sha256_hasher::hashv;
use wincode::{SchemaRead, SchemaWrite};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionHashError {
    InstructionSliceOutOfBounds,
    AccountIndexOutOfBounds,
    InvalidInstructionData,
}

pub fn compute_action_hash_from_metas(
    program_id: &Pubkey,
    ops: &Ops,
    accounts: &[AccountMeta],
    ix_data: &[u8],
) -> Result<[u8; 32], ActionHashError> {
    let mut chunks = vec![program_id.to_bytes().to_vec()];

    for op in &ops.ops {
        match op {
            Op::Noop => chunks.push(vec![0]),
            Op::IngestInstruction { offset, len } => {
                let start = usize::from(*offset);
                let length = usize::from(*len);
                let end = start
                    .checked_add(length)
                    .ok_or(ActionHashError::InstructionSliceOutOfBounds)?;
                let slice = ix_data
                    .get(start..end)
                    .ok_or(ActionHashError::InstructionSliceOutOfBounds)?;

                chunks.push(vec![1]);
                chunks.push(offset.to_le_bytes().to_vec());
                chunks.push(vec![*len]);
                chunks.push(slice.to_vec());
            }
            Op::IngestAccount { index } => {
                let account = accounts
                    .get(usize::from(*index))
                    .ok_or(ActionHashError::AccountIndexOutOfBounds)?;

                chunks.push(vec![2]);
                chunks.push(vec![*index]);
                chunks.push(account.pubkey.to_bytes().to_vec());
                chunks.push(vec![u8::from(account.is_signer)]);
                chunks.push(vec![u8::from(account.is_writable)]);
            }
            Op::IngestInstructionDataSize => {
                let data_len = u32::try_from(ix_data.len())
                    .map_err(|_| ActionHashError::InvalidInstructionData)?;

                chunks.push(vec![3]);
                chunks.push(data_len.to_le_bytes().to_vec());
            }
        }
    }

    let refs = chunks.iter().map(Vec::as_slice).collect::<Vec<_>>();
    Ok(hashv(&refs).to_bytes())
}
