use solana_instruction::AccountMeta;
use solana_pubkey::Pubkey;
use solana_sha256_hasher::hashv;
use wincode::{SchemaRead, SchemaWrite};

/// Maximum number of authorization predicates stored on one Action account.
///
/// Each stored op is four bytes, so 32 ops reserve 128 bytes inside the fixed
/// Action account layout. More complex workflows should split across multiple
/// authorized actions rather than growing this account dynamically.
pub const MAX_ACTION_OPS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
#[wincode(tag_encoding = "u8")]
pub enum ActionScope {
    #[wincode(tag = 0)]
    Manager,
    #[wincode(tag = 1)]
    Swap,
    #[wincode(tag = 2)]
    Public,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct StoredOp {
    pub kind: u8,
    pub arg0: u8,
    pub arg1: u8,
    pub arg2: u8,
}

impl StoredOp {
    pub const fn noop() -> Self {
        Self {
            kind: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        }
    }

    fn try_to_op(self) -> Result<Op, ActionHashError> {
        let op = match self.kind {
            0 if self.arg0 == 0 && self.arg1 == 0 && self.arg2 == 0 => Op::Noop,
            1 => Op::IngestInstruction {
                offset: u16::from_le_bytes([self.arg1, self.arg2]),
                len: self.arg0,
            },
            2 if self.arg1 == 0 && self.arg2 == 0 => Op::IngestAccount { index: self.arg0 },
            3 if self.arg0 == 0 && self.arg1 == 0 && self.arg2 == 0 => {
                Op::IngestInstructionDataSize
            }
            _ => return Err(ActionHashError::InvalidOp),
        };

        Ok(op)
    }
}

impl Default for StoredOp {
    fn default() -> Self {
        Self::noop()
    }
}

impl From<Op> for StoredOp {
    fn from(op: Op) -> Self {
        match op {
            Op::Noop => Self::noop(),
            Op::IngestInstruction { offset, len } => {
                let [arg1, arg2] = offset.to_le_bytes();
                Self {
                    kind: 1,
                    arg0: len,
                    arg1,
                    arg2,
                }
            }
            Op::IngestAccount { index } => Self {
                kind: 2,
                arg0: index,
                arg1: 0,
                arg2: 0,
            },
            Op::IngestInstructionDataSize => Self {
                kind: 3,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct Ops {
    pub ops: [StoredOp; 32],
    pub ops_len: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincode::serialize;

    #[test]
    fn stored_ops_are_compact() {
        assert_eq!(core::mem::size_of::<StoredOp>(), 4);
        assert_eq!(core::mem::size_of::<Ops>(), MAX_ACTION_OPS * 4 + 1);
        assert_eq!(serialize(&StoredOp::noop()).unwrap().len(), 4);
        assert_eq!(
            serialize(&Ops::empty()).unwrap().len(),
            MAX_ACTION_OPS * 4 + 1
        );
    }

    #[test]
    fn ops_new_enforces_capacity() {
        let ops = vec![Op::Noop; MAX_ACTION_OPS + 1];

        assert_eq!(Ops::new(ops), Err(ActionHashError::TooManyOps));
    }

    #[test]
    fn ops_round_trip_logical_ops() {
        let ops = Ops::new([
            Op::Noop,
            Op::IngestInstruction {
                offset: 513,
                len: 7,
            },
            Op::IngestAccount { index: 9 },
            Op::IngestInstructionDataSize,
        ])
        .unwrap();
        let decoded = ops.iter().unwrap().collect::<Result<Vec<_>, _>>().unwrap();

        assert_eq!(
            decoded,
            vec![
                Op::Noop,
                Op::IngestInstruction {
                    offset: 513,
                    len: 7,
                },
                Op::IngestAccount { index: 9 },
                Op::IngestInstructionDataSize,
            ]
        );
    }

    #[test]
    fn hash_rejects_corrupt_stored_ops() {
        let program_id = Pubkey::new_unique();
        let account_metas = [];
        let ix_data = [];

        let mut too_many = Ops::empty();
        too_many.ops_len = u8::try_from(MAX_ACTION_OPS + 1).unwrap();
        assert_eq!(
            compute_action_hash_from_metas(&program_id, &too_many, &account_metas, &ix_data),
            Err(ActionHashError::TooManyOps)
        );

        let mut invalid_kind = Ops::empty();
        invalid_kind.ops[0] = StoredOp {
            kind: 255,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        };
        invalid_kind.ops_len = 1;
        assert_eq!(
            compute_action_hash_from_metas(&program_id, &invalid_kind, &account_metas, &ix_data),
            Err(ActionHashError::InvalidOp)
        );

        let mut non_canonical = Ops::empty();
        non_canonical.ops[0] = StoredOp {
            kind: 0,
            arg0: 1,
            arg1: 0,
            arg2: 0,
        };
        non_canonical.ops_len = 1;
        assert_eq!(
            compute_action_hash_from_metas(&program_id, &non_canonical, &account_metas, &ix_data),
            Err(ActionHashError::InvalidOp)
        );
    }
}

impl Ops {
    pub const fn empty() -> Self {
        Self {
            ops: [StoredOp::noop(); MAX_ACTION_OPS],
            ops_len: 0,
        }
    }

    pub fn new(ops: impl IntoIterator<Item = Op>) -> Result<Self, ActionHashError> {
        let mut stored_ops = Self::empty();

        for (index, op) in ops.into_iter().enumerate() {
            if index >= MAX_ACTION_OPS {
                return Err(ActionHashError::TooManyOps);
            }

            stored_ops.ops[index] = StoredOp::from(op);
            stored_ops.ops_len =
                u8::try_from(index + 1).map_err(|_| ActionHashError::InvalidInstructionData)?;
        }

        Ok(stored_ops)
    }

    pub fn len(&self) -> Result<usize, ActionHashError> {
        let len = usize::from(self.ops_len);
        if len > MAX_ACTION_OPS {
            return Err(ActionHashError::TooManyOps);
        }

        Ok(len)
    }

    pub fn is_empty(&self) -> Result<bool, ActionHashError> {
        Ok(self.len()? == 0)
    }

    pub fn iter(
        &self,
    ) -> Result<impl Iterator<Item = Result<Op, ActionHashError>> + '_, ActionHashError> {
        let len = self.len()?;
        Ok(self.ops[..len].iter().copied().map(StoredOp::try_to_op))
    }
}

impl Default for Ops {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionHashError {
    InvalidOp,
    TooManyOps,
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

    for op in ops.iter()? {
        match op? {
            Op::Noop => chunks.push(vec![0]),
            Op::IngestInstruction { offset, len } => {
                let start = usize::from(offset);
                let length = usize::from(len);
                let end = start
                    .checked_add(length)
                    .ok_or(ActionHashError::InstructionSliceOutOfBounds)?;
                let slice = ix_data
                    .get(start..end)
                    .ok_or(ActionHashError::InstructionSliceOutOfBounds)?;

                chunks.push(vec![1]);
                chunks.push(offset.to_le_bytes().to_vec());
                chunks.push(vec![len]);
                chunks.push(slice.to_vec());
            }
            Op::IngestAccount { index } => {
                let account = accounts
                    .get(usize::from(index))
                    .ok_or(ActionHashError::AccountIndexOutOfBounds)?;

                chunks.push(vec![2]);
                chunks.push(vec![index]);
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
