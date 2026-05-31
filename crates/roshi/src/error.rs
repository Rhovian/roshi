use solana_program_error::ProgramError;

#[repr(u32)]
pub enum RoshiError {
    InvalidOp = 0,
    InstructionSliceOutOfBounds = 1,
    AccountIndexOutOfBounds = 2,
    InvalidBps = 3,
    VaultPaused = 4,
    UnauthorizedAction = 5,
    InvalidProgramConfigAccount = 6,
    InvalidVaultAccount = 7,
    InvalidActionAccount = 8,
    InvalidWithdrawalTicketAccount = 9,
    InvalidAssetAccount = 10,
    InvalidAccessProof = 11,
    InvalidVaultTag = 12,
}

impl From<RoshiError> for ProgramError {
    fn from(error: RoshiError) -> Self {
        ProgramError::Custom(error as u32)
    }
}
