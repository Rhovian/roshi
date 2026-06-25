use solana_program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    DivisionByZero = 13,
    InvalidDecimals = 14,
    InvalidVaultState = 15,
    Overflow = 16,
    ResultDoesNotFit = 17,
    ZeroOutput = 18,
    SlippageExceeded = 19,
    InvalidTokenAccount = 20,
    InvalidMintAccount = 21,
    ExternalDisabled = 22,
    WithdrawalExceedsEntitlement = 23,
    InsufficientShares = 24,
    InvalidSubAccount = 25,
    InvalidExternalDestinationAccount = 26,
    InvalidWriteDownAmount = 27,
    StaleNavReport = 28,
    NavGainExceedsBound = 29,
    ReportTooFrequent = 30,
    DepositCapExceeded = 31,
    ExternalDestinationNotRegistered = 32,
    UnpriceableSwapLeg = 33,
    MissingInstructionsSysvar = 34,
    RequiredSiblingMissing = 35,
    FlashDelegateUnbounded = 36,
    FlashDelegateMismatch = 37,
    FlashDestinationMismatch = 38,
    SiblingsRequireTopLevel = 39,
    DelegateNotCleared = 40,
    FlashFeeRateNotLower = 41,
    SwapCustodyMoved = 42,
    EmptyAtomicRedeemOps = 43,
    UnboundAtomicRedeemAccount = 44,
}

impl From<RoshiError> for ProgramError {
    fn from(error: RoshiError) -> Self {
        ProgramError::Custom(error as u32)
    }
}
