use crate::{
    action::{ActionScope, Ops},
    oracle::OracleConfig,
    state::VaultControls,
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
    pub controls: VaultControls,
    pub private: bool,
    pub access_merkle_root: [u8; 32],
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct AuthorizeActionArgs {
    pub action_hash: [u8; 32],
    pub scope: ActionScope,
    pub ops: Ops,
    pub redeem_amount_offset: u16,
    /// `FlashApprove` flash-fee rate as an opaque committed fraction (#21);
    /// stored on the `Action` but not part of `action_hash`. `fee_num == 0` is
    /// a fee-free action (any other scope ignores these).
    pub fee_num: u64,
    pub fee_den: u64,
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
pub struct SwapArgs {
    pub min_out: u64,
    pub max_in: u64,
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
    /// Marked base-atom value of everything the program does *not* read as idle:
    /// venue positions, non-base idle, and any base not held in the vault's
    /// current deposit/withdraw custodies (e.g. base stranded in a sub-account
    /// after a repoint — accounting for it is the off-chain NAV's job). The
    /// program reads idle base on-chain and forms gross NAV = idle +
    /// `external_value`.
    pub external_value: u64,
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
    pub controls: VaultControls,
    pub external_enabled: bool,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct InitializeAssetArgs {
    pub asset_mint: [u8; 32],
    pub oracle: OracleConfig,
    pub asset_decimals: u8,
    pub enabled: bool,
    /// Price deposits as `oracle / vault.base_oracle` (two legs sharing a
    /// quote currency) instead of reading `oracle` as a direct asset/base feed.
    pub routed: bool,
    /// Inventory cap in asset atoms: deposits rejecting once
    /// `custody_balance + amount` would exceed it. `u64::MAX` = uncapped.
    pub deposit_cap_atoms: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct UpdateAssetArgs {
    pub oracle: OracleConfig,
    pub enabled: bool,
    pub routed: bool,
    pub deposit_cap_atoms: u64,
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

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct SetShareMetadataArgs {
    /// Display metadata for the share mint, stored via Metaplex Token
    /// Metadata (length limits are enforced by the Metaplex program: name
    /// <= 32, symbol <= 10, uri <= 200). Display only — no economic
    /// invariant may depend on it.
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct WriteDownFeesArgs {
    /// Fee liability to forgive: `0 < amount <= fees_payable`. No tokens
    /// move; gross NAV is unchanged and liabilities shrink.
    pub amount: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct AssertDelegateClearedArgs;

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct AdminSetFlashFeeRateArgs {
    pub fee_num: u64,
    pub fee_den: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct StrategistLowerFlashFeeRateArgs {
    pub fee_num: u64,
    pub fee_den: u64,
}

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct RegisterExternalDestinationArgs;

#[derive(codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct RevokeExternalDestinationArgs;
