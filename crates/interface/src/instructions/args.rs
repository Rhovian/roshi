use crate::{
    action::{ActionScope, Ops},
    oracle::OracleConfig,
};
use wincode::{SchemaRead, SchemaWrite};

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct InitializeProgramArgs {
    pub authority: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct InitializeVaultArgs {
    pub tag: [u8; 32],
    pub tag_len: u8,
    pub admin: [u8; 32],
    pub strategist: [u8; 32],
    pub swap_authority: [u8; 32],
    pub nav_authority: [u8; 32],
    pub withdrawal_authority: [u8; 32],
    pub base_mint: [u8; 32],
    pub base_decimals: u8,
    pub base_oracle: OracleConfig,
    pub deposit_sub_account: u8,
    pub withdraw_sub_account: u8,
    pub treasury: [u8; 32],
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub private: bool,
    pub access_merkle_root: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct AuthorizeActionArgs {
    pub action_hash: [u8; 32],
    pub scope: ActionScope,
    pub ops: Ops,
    pub redeem_amount_offset: u16,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct RevokeActionArgs {
    pub action_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct AccountFlags {
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct ManageArgs {
    pub sub_account: u8,
    pub program_id: [u8; 32],
    pub accounts_start: u8,
    pub accounts_len: u8,
    pub account_flags: Vec<AccountFlags>,
    pub ix_data: Vec<u8>,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct AtomicRedeemArgs {
    pub shares: u64,
    pub min_output: u64,
    pub sub_account: u8,
    pub program_id: [u8; 32],
    pub accounts_start: u8,
    pub accounts_len: u8,
    pub account_flags: Vec<AccountFlags>,
    pub ix_data: Vec<u8>,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct ManageBatchArgs {
    pub actions: Vec<ManageArgs>,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct DepositArgs {
    pub asset_mint: [u8; 32],
    pub amount: u64,
    pub min_shares_out: u64,
    pub access_proof: Vec<[u8; 32]>,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct RedeemArgs {
    pub recipient_token_account: [u8; 32],
    pub ticket_index: u8,
    pub shares: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct CancelRedeemArgs {
    pub min_shares_out: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct ProcessWithdrawalsArgs;

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct InvestExternalArgs {
    pub sub_account: u8,
    pub amount: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct ReturnExternalArgs {
    pub sub_account: u8,
    pub amount: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct CollectFeesArgs {
    pub sub_account: u8,
    pub amount: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct ReportNavArgs {
    pub total_assets: u64,
    pub report_hash: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct UpdateVaultConfigArgs {
    pub treasury: [u8; 32],
    pub deposit_sub_account: u8,
    pub withdraw_sub_account: u8,
    pub base_oracle: OracleConfig,
    pub performance_fee_bps: u16,
    pub withdrawal_buffer_bps: u16,
    pub external_enabled: bool,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct InitializeAssetArgs {
    pub asset_mint: [u8; 32],
    pub oracle: OracleConfig,
    pub asset_decimals: u8,
    pub enabled: bool,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct UpdateAssetArgs {
    pub oracle: OracleConfig,
    pub enabled: bool,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct SetPauseFlagsArgs {
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
    pub manage_paused: bool,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct SetVaultAccessArgs {
    pub private: bool,
    pub access_merkle_root: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct TransferProgramAuthorityArgs {
    pub new_authority: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct TransferVaultAuthorityArgs {
    pub new_authority: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct SetStrategistArgs {
    pub strategist: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct SetSwapAuthorityArgs {
    pub swap_authority: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct SetNavAuthorityArgs {
    pub nav_authority: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct SetWithdrawalAuthorityArgs {
    pub withdrawal_authority: [u8; 32],
}
