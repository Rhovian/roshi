pub(crate) mod accounts;
pub(crate) mod token;

pub mod admin;
pub mod execution;
pub mod user;

pub use roshi_interface::instructions::{
    AccountFlags, AtomicRedeemArgs, AuthorizeActionArgs, CancelRedeemArgs, CollectFeesArgs,
    DepositArgs, InitializeAssetArgs, InitializeProgramArgs, InitializeVaultArgs,
    InvestExternalArgs, ManageArgs, ManageBatchArgs, ProcessWithdrawalsArgs, RedeemArgs,
    RegisterExternalDestinationArgs, ReportNavArgs, ReturnExternalArgs, RevokeActionArgs,
    RevokeExternalDestinationArgs, RoshiInstruction, SetNavAuthorityArgs, SetPauseFlagsArgs,
    SetStrategistArgs, SetSwapAuthorityArgs, SetVaultAccessArgs, SetWithdrawalAuthorityArgs,
    SwapArgs, TransferProgramAuthorityArgs, TransferVaultAuthorityArgs, UpdateAssetArgs,
    UpdateVaultConfigArgs, WriteDownFeesArgs,
};
