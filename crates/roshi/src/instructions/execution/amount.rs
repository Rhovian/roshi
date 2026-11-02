use crate::state::action::Action;
use roshi_interface::error::RoshiError;
use solana_program_error::ProgramError;

/// Read the route amount at its admin-configured byte offset.
pub(crate) fn decode_withdrawal_amount(
    ix_data: &[u8],
    action: &Action,
) -> Result<u64, ProgramError> {
    let start = usize::from(action.amount_offset);
    let end = start
        .checked_add(8)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let bytes = ix_data
        .get(start..end)
        .ok_or(ProgramError::from(RoshiError::InstructionSliceOutOfBounds))?;

    Ok(u64::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    ))
}
