pub mod args;

pub use args::*;

use wincode::{config::DefaultConfig, SchemaRead, SchemaWrite};

pub trait InstructionArgs: SchemaWrite<DefaultConfig, Src = Self> {
    const TAG: u8;
}

pub mod tags {
    pub const INITIALIZE_PROGRAM: u8 = 0;
    pub const INITIALIZE_VAULT: u8 = 1;
    pub const AUTHORIZE_ACTION: u8 = 2;
    pub const REVOKE_ACTION: u8 = 3;
    pub const MANAGE: u8 = 4;
    pub const MANAGE_BATCH: u8 = 5;
    pub const REPORT_NAV: u8 = 6;
    pub const DEPOSIT: u8 = 7;
    pub const REDEEM: u8 = 8;
    pub const CANCEL_REDEEM: u8 = 9;
    pub const PROCESS_WITHDRAWALS: u8 = 10;
    pub const UPDATE_VAULT_CONFIG: u8 = 11;
    pub const INITIALIZE_ASSET: u8 = 12;
    pub const UPDATE_ASSET: u8 = 13;
    pub const SET_PAUSE_FLAGS: u8 = 14;
    pub const SET_VAULT_ACCESS: u8 = 15;
    pub const TRANSFER_PROGRAM_AUTHORITY: u8 = 16;
    pub const TRANSFER_VAULT_AUTHORITY: u8 = 17;
    pub const SET_STRATEGIST: u8 = 18;
    pub const SET_NAV_AUTHORITY: u8 = 19;
    pub const SET_WITHDRAWAL_AUTHORITY: u8 = 20;
    pub const COLLECT_FEES: u8 = 21;
    pub const INVEST_EXTERNAL: u8 = 22;
    pub const RETURN_EXTERNAL: u8 = 23;
    pub const SET_SWAP_AUTHORITY: u8 = 24;
    pub const ATOMIC_REDEEM: u8 = 25;
    pub const SWAP: u8 = 26;
}

// Codama parses the enum source directly and currently requires literal
// discriminator values here; keep `tags::*` in sync via the IDL test below.
#[repr(u8)]
#[derive(codama_macros::CodamaInstructions)]
#[allow(clippy::large_enum_variant)]
#[codama(program(
    name = "roshi",
    address = "Roshi11111111111111111111111111111111111111"
))]
pub enum RoshiInstruction {
    #[codama(account(name = "payer", signer, writable))]
    #[codama(account(name = "program_config", writable))]
    #[codama(account(name = "system_program", default_value = program("system")))]
    InitializeProgram(#[codama(name = "args")] InitializeProgramArgs) = 0,

    #[codama(account(name = "program_authority", signer))]
    #[codama(account(name = "program_config"))]
    #[codama(account(name = "payer", signer, writable))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "base_mint"))]
    #[codama(account(name = "share_mint", writable))]
    #[codama(account(name = "treasury"))]
    #[codama(account(name = "system_program", default_value = program("system")))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    InitializeVault(#[codama(name = "args")] InitializeVaultArgs) = 1,

    #[codama(account(name = "admin", signer, writable))]
    #[codama(account(name = "vault"))]
    #[codama(account(name = "action", writable))]
    #[codama(account(name = "system_program", default_value = program("system")))]
    AuthorizeAction(#[codama(name = "args")] AuthorizeActionArgs) = 2,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault"))]
    #[codama(account(name = "action", writable))]
    RevokeAction(#[codama(name = "args")] RevokeActionArgs) = 3,

    #[codama(account(name = "executor", signer))]
    #[codama(account(name = "vault"))]
    #[codama(account(name = "sub_account", writable))]
    #[codama(account(name = "action"))]
    Manage(#[codama(name = "args")] ManageArgs) = 4,

    #[codama(account(name = "executor", signer))]
    #[codama(account(name = "vault"))]
    ManageBatch(#[codama(name = "args")] ManageBatchArgs) = 5,

    #[codama(account(name = "nav_authority", signer))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "share_mint"))]
    #[codama(account(name = "base_mint"))]
    #[codama(account(name = "deposit_base_custody"))]
    #[codama(account(name = "withdraw_base_custody"))]
    ReportNav(#[codama(name = "args")] ReportNavArgs) = 6,

    #[codama(account(name = "depositor", signer))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "user_source_token_account", writable))]
    #[codama(account(name = "vault_custody_token_account", writable))]
    #[codama(account(name = "user_share_account", writable))]
    #[codama(account(name = "share_mint", writable))]
    #[codama(account(name = "share_token_program", default_value = program("token")))]
    #[codama(account(name = "asset_token_program"))]
    Deposit(#[codama(name = "args")] DepositArgs) = 7,

    #[codama(account(name = "owner", signer, writable))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "user_share_account", writable))]
    #[codama(account(name = "share_mint", writable))]
    #[codama(account(name = "recipient_token_account"))]
    #[codama(account(name = "withdrawal_ticket", writable))]
    #[codama(account(name = "system_program", default_value = program("system")))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    Redeem(#[codama(name = "args")] RedeemArgs) = 8,

    #[codama(account(name = "owner", signer, writable))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "withdrawal_ticket", writable))]
    #[codama(account(name = "share_mint", writable))]
    #[codama(account(name = "owner_share_account", writable))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    CancelRedeem(#[codama(name = "args")] CancelRedeemArgs) = 9,

    #[codama(account(name = "withdrawal_authority", signer))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "withdraw_sub_account"))]
    #[codama(account(name = "custody", writable))]
    #[codama(account(name = "share_mint"))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    ProcessWithdrawals = 10,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "treasury"))]
    UpdateVaultConfig(#[codama(name = "args")] UpdateVaultConfigArgs) = 11,

    #[codama(account(name = "admin", signer, writable))]
    #[codama(account(name = "vault"))]
    #[codama(account(name = "asset_mint"))]
    #[codama(account(name = "asset", writable))]
    #[codama(account(name = "system_program", default_value = program("system")))]
    InitializeAsset(#[codama(name = "args")] InitializeAssetArgs) = 12,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault"))]
    #[codama(account(name = "asset", writable))]
    UpdateAsset(#[codama(name = "args")] UpdateAssetArgs) = 13,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    SetPauseFlags(#[codama(name = "args")] SetPauseFlagsArgs) = 14,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    SetVaultAccess(#[codama(name = "args")] SetVaultAccessArgs) = 15,

    #[codama(account(name = "authority", signer))]
    #[codama(account(name = "program_config", writable))]
    TransferProgramAuthority(#[codama(name = "args")] TransferProgramAuthorityArgs) = 16,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    TransferVaultAuthority(#[codama(name = "args")] TransferVaultAuthorityArgs) = 17,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    SetStrategist(#[codama(name = "args")] SetStrategistArgs) = 18,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    SetNavAuthority(#[codama(name = "args")] SetNavAuthorityArgs) = 19,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    SetWithdrawalAuthority(#[codama(name = "args")] SetWithdrawalAuthorityArgs) = 20,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "fee_sub_account"))]
    #[codama(account(name = "custody", writable))]
    #[codama(account(name = "treasury", writable))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    CollectFees(#[codama(name = "args")] CollectFeesArgs) = 21,

    #[codama(account(name = "strategist", signer))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "sub_account"))]
    #[codama(account(name = "custody", writable))]
    #[codama(account(name = "external_account", writable))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    InvestExternal(#[codama(name = "args")] InvestExternalArgs) = 22,

    #[codama(account(name = "strategist", signer))]
    #[codama(account(name = "external_authority", signer))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "sub_account"))]
    #[codama(account(name = "external_account", writable))]
    #[codama(account(name = "custody", writable))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    ReturnExternal(#[codama(name = "args")] ReturnExternalArgs) = 23,

    #[codama(account(name = "admin", signer))]
    #[codama(account(name = "vault", writable))]
    SetSwapAuthority(#[codama(name = "args")] SetSwapAuthorityArgs) = 24,

    #[codama(account(name = "owner", signer, writable))]
    #[codama(account(name = "vault", writable))]
    #[codama(account(name = "user_share_account", writable))]
    #[codama(account(name = "share_mint", writable))]
    #[codama(account(name = "recipient_token_account", writable))]
    #[codama(account(name = "custody", writable))]
    #[codama(account(name = "base_token_program"))]
    #[codama(account(name = "sub_account"))]
    #[codama(account(name = "action"))]
    #[codama(account(name = "token_program", default_value = program("token")))]
    AtomicRedeem(#[codama(name = "args")] AtomicRedeemArgs) = 25,

    #[codama(account(name = "swap_authority", signer))]
    #[codama(account(name = "vault"))]
    #[codama(account(name = "sub_account"))]
    #[codama(account(name = "input_custody", writable))]
    #[codama(account(name = "output_custody", writable))]
    #[codama(account(name = "action"))]
    Swap(#[codama(name = "args")] SwapArgs) = 26,
}

impl RoshiInstruction {
    pub const fn tag(&self) -> u8 {
        match self {
            Self::InitializeProgram(_) => tags::INITIALIZE_PROGRAM,
            Self::InitializeVault(_) => tags::INITIALIZE_VAULT,
            Self::AuthorizeAction(_) => tags::AUTHORIZE_ACTION,
            Self::RevokeAction(_) => tags::REVOKE_ACTION,
            Self::Manage(_) => tags::MANAGE,
            Self::ManageBatch(_) => tags::MANAGE_BATCH,
            Self::ReportNav(_) => tags::REPORT_NAV,
            Self::Deposit(_) => tags::DEPOSIT,
            Self::Redeem(_) => tags::REDEEM,
            Self::CancelRedeem(_) => tags::CANCEL_REDEEM,
            Self::ProcessWithdrawals => tags::PROCESS_WITHDRAWALS,
            Self::UpdateVaultConfig(_) => tags::UPDATE_VAULT_CONFIG,
            Self::InitializeAsset(_) => tags::INITIALIZE_ASSET,
            Self::UpdateAsset(_) => tags::UPDATE_ASSET,
            Self::InvestExternal(_) => tags::INVEST_EXTERNAL,
            Self::ReturnExternal(_) => tags::RETURN_EXTERNAL,
            Self::SetPauseFlags(_) => tags::SET_PAUSE_FLAGS,
            Self::SetVaultAccess(_) => tags::SET_VAULT_ACCESS,
            Self::TransferProgramAuthority(_) => tags::TRANSFER_PROGRAM_AUTHORITY,
            Self::TransferVaultAuthority(_) => tags::TRANSFER_VAULT_AUTHORITY,
            Self::SetStrategist(_) => tags::SET_STRATEGIST,
            Self::SetSwapAuthority(_) => tags::SET_SWAP_AUTHORITY,
            Self::SetNavAuthority(_) => tags::SET_NAV_AUTHORITY,
            Self::SetWithdrawalAuthority(_) => tags::SET_WITHDRAWAL_AUTHORITY,
            Self::CollectFees(_) => tags::COLLECT_FEES,
            Self::AtomicRedeem(_) => tags::ATOMIC_REDEEM,
            Self::Swap(_) => tags::SWAP,
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn decode(data: &[u8]) -> Result<Self, ()> {
        let (tag, payload) = data.split_first().ok_or(())?;

        match *tag {
            tags::INITIALIZE_PROGRAM => Ok(Self::InitializeProgram(decode_payload(payload)?)),
            tags::INITIALIZE_VAULT => Ok(Self::InitializeVault(decode_payload(payload)?)),
            tags::AUTHORIZE_ACTION => Ok(Self::AuthorizeAction(decode_payload(payload)?)),
            tags::REVOKE_ACTION => Ok(Self::RevokeAction(decode_payload(payload)?)),
            tags::MANAGE => Ok(Self::Manage(decode_payload(payload)?)),
            tags::MANAGE_BATCH => Ok(Self::ManageBatch(decode_payload(payload)?)),
            tags::REPORT_NAV => Ok(Self::ReportNav(decode_payload(payload)?)),
            tags::DEPOSIT => Ok(Self::Deposit(decode_payload(payload)?)),
            tags::REDEEM => Ok(Self::Redeem(decode_payload(payload)?)),
            tags::CANCEL_REDEEM => Ok(Self::CancelRedeem(decode_payload(payload)?)),
            tags::PROCESS_WITHDRAWALS => {
                let ProcessWithdrawalsArgs = decode_payload(payload)?;
                Ok(Self::ProcessWithdrawals)
            }
            tags::UPDATE_VAULT_CONFIG => Ok(Self::UpdateVaultConfig(decode_payload(payload)?)),
            tags::INITIALIZE_ASSET => Ok(Self::InitializeAsset(decode_payload(payload)?)),
            tags::UPDATE_ASSET => Ok(Self::UpdateAsset(decode_payload(payload)?)),
            tags::INVEST_EXTERNAL => Ok(Self::InvestExternal(decode_payload(payload)?)),
            tags::RETURN_EXTERNAL => Ok(Self::ReturnExternal(decode_payload(payload)?)),
            tags::SET_PAUSE_FLAGS => Ok(Self::SetPauseFlags(decode_payload(payload)?)),
            tags::SET_VAULT_ACCESS => Ok(Self::SetVaultAccess(decode_payload(payload)?)),
            tags::TRANSFER_PROGRAM_AUTHORITY => {
                Ok(Self::TransferProgramAuthority(decode_payload(payload)?))
            }
            tags::TRANSFER_VAULT_AUTHORITY => {
                Ok(Self::TransferVaultAuthority(decode_payload(payload)?))
            }
            tags::SET_STRATEGIST => Ok(Self::SetStrategist(decode_payload(payload)?)),
            tags::SET_SWAP_AUTHORITY => Ok(Self::SetSwapAuthority(decode_payload(payload)?)),
            tags::SET_NAV_AUTHORITY => Ok(Self::SetNavAuthority(decode_payload(payload)?)),
            tags::SET_WITHDRAWAL_AUTHORITY => {
                Ok(Self::SetWithdrawalAuthority(decode_payload(payload)?))
            }
            tags::COLLECT_FEES => Ok(Self::CollectFees(decode_payload(payload)?)),
            tags::ATOMIC_REDEEM => Ok(Self::AtomicRedeem(decode_payload(payload)?)),
            tags::SWAP => Ok(Self::Swap(decode_payload(payload)?)),
            _ => Err(()),
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, wincode::WriteError> {
        let mut data = vec![self.tag()];

        match self {
            Self::InitializeProgram(args) => wincode::serialize_into(&mut data, args)?,
            Self::InitializeVault(args) => wincode::serialize_into(&mut data, args)?,
            Self::AuthorizeAction(args) => wincode::serialize_into(&mut data, args)?,
            Self::RevokeAction(args) => wincode::serialize_into(&mut data, args)?,
            Self::Manage(args) => wincode::serialize_into(&mut data, args)?,
            Self::ManageBatch(args) => wincode::serialize_into(&mut data, args)?,
            Self::ReportNav(args) => wincode::serialize_into(&mut data, args)?,
            Self::Deposit(args) => wincode::serialize_into(&mut data, args)?,
            Self::Redeem(args) => wincode::serialize_into(&mut data, args)?,
            Self::CancelRedeem(args) => wincode::serialize_into(&mut data, args)?,
            Self::ProcessWithdrawals => {
                wincode::serialize_into(&mut data, &ProcessWithdrawalsArgs)?
            }
            Self::UpdateVaultConfig(args) => wincode::serialize_into(&mut data, args)?,
            Self::InitializeAsset(args) => wincode::serialize_into(&mut data, args)?,
            Self::UpdateAsset(args) => wincode::serialize_into(&mut data, args)?,
            Self::InvestExternal(args) => wincode::serialize_into(&mut data, args)?,
            Self::ReturnExternal(args) => wincode::serialize_into(&mut data, args)?,
            Self::SetPauseFlags(args) => wincode::serialize_into(&mut data, args)?,
            Self::SetVaultAccess(args) => wincode::serialize_into(&mut data, args)?,
            Self::TransferProgramAuthority(args) => wincode::serialize_into(&mut data, args)?,
            Self::TransferVaultAuthority(args) => wincode::serialize_into(&mut data, args)?,
            Self::SetStrategist(args) => wincode::serialize_into(&mut data, args)?,
            Self::SetSwapAuthority(args) => wincode::serialize_into(&mut data, args)?,
            Self::SetNavAuthority(args) => wincode::serialize_into(&mut data, args)?,
            Self::SetWithdrawalAuthority(args) => wincode::serialize_into(&mut data, args)?,
            Self::CollectFees(args) => wincode::serialize_into(&mut data, args)?,
            Self::AtomicRedeem(args) => wincode::serialize_into(&mut data, args)?,
            Self::Swap(args) => wincode::serialize_into(&mut data, args)?,
        }

        Ok(data)
    }
}

fn decode_payload<'a, T>(payload: &'a [u8]) -> Result<T, ()>
where
    T: SchemaRead<'a, DefaultConfig, Dst = T>,
{
    wincode::deserialize_exact(payload).map_err(|_| ())
}

macro_rules! impl_instruction_args {
    ($( $args:ty = $tag:expr ),+ $(,)?) => {
        $(
            impl InstructionArgs for $args {
                const TAG: u8 = $tag;
            }
        )+

        #[cfg(test)]
        const TAG_CASES: &[u8] = &[
            $(
                $tag,
            )+
        ];
    };
}

impl_instruction_args! {
    InitializeProgramArgs = tags::INITIALIZE_PROGRAM,
    InitializeVaultArgs = tags::INITIALIZE_VAULT,
    AuthorizeActionArgs = tags::AUTHORIZE_ACTION,
    RevokeActionArgs = tags::REVOKE_ACTION,
    ManageArgs = tags::MANAGE,
    ManageBatchArgs = tags::MANAGE_BATCH,
    ReportNavArgs = tags::REPORT_NAV,
    DepositArgs = tags::DEPOSIT,
    RedeemArgs = tags::REDEEM,
    CancelRedeemArgs = tags::CANCEL_REDEEM,
    ProcessWithdrawalsArgs = tags::PROCESS_WITHDRAWALS,
    UpdateVaultConfigArgs = tags::UPDATE_VAULT_CONFIG,
    InitializeAssetArgs = tags::INITIALIZE_ASSET,
    UpdateAssetArgs = tags::UPDATE_ASSET,
    SetPauseFlagsArgs = tags::SET_PAUSE_FLAGS,
    SetVaultAccessArgs = tags::SET_VAULT_ACCESS,
    TransferProgramAuthorityArgs = tags::TRANSFER_PROGRAM_AUTHORITY,
    TransferVaultAuthorityArgs = tags::TRANSFER_VAULT_AUTHORITY,
    SetStrategistArgs = tags::SET_STRATEGIST,
    SetNavAuthorityArgs = tags::SET_NAV_AUTHORITY,
    SetWithdrawalAuthorityArgs = tags::SET_WITHDRAWAL_AUTHORITY,
    CollectFeesArgs = tags::COLLECT_FEES,
    InvestExternalArgs = tags::INVEST_EXTERNAL,
    ReturnExternalArgs = tags::RETURN_EXTERNAL,
    SetSwapAuthorityArgs = tags::SET_SWAP_AUTHORITY,
    AtomicRedeemArgs = tags::ATOMIC_REDEEM,
    SwapArgs = tags::SWAP,
}

pub fn serialize_instruction<T>(args: &T) -> Result<Vec<u8>, wincode::WriteError>
where
    T: InstructionArgs,
{
    let mut data = vec![T::TAG];
    wincode::serialize_into(&mut data, args)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codama::{Codama, NodeTrait};
    use serde_json::Value;
    use std::path::Path;
    use wincode::deserialize_exact;

    #[test]
    fn instruction_args_tags_match_canonical_tags() {
        assert_eq!(
            TAG_CASES,
            &[
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                23, 24, 25, 26
            ]
        );
        assert_eq!(
            RoshiInstruction::ProcessWithdrawals.tag(),
            <ProcessWithdrawalsArgs as InstructionArgs>::TAG
        );
    }

    #[test]
    fn codama_idl_uses_canonical_instruction_discriminators() {
        let mut idl = Codama::load(Path::new(env!("CARGO_MANIFEST_DIR")))
            .unwrap()
            .get_idl()
            .unwrap();
        idl.program.name = "roshi".into();
        let idl: Value = serde_json::from_str(&idl.to_json().unwrap()).unwrap();
        let instructions = idl["program"]["instructions"].as_array().unwrap();

        assert_eq!(idl["program"]["name"], "roshi");
        assert_eq!(
            idl["program"]["publicKey"],
            "Roshi11111111111111111111111111111111111111"
        );
        assert_eq!(instructions.len(), TAG_CASES.len());

        for (name, tag) in IDL_TAG_CASES {
            assert_instruction_discriminator(instructions, name, *tag);
        }

        let deposit = instruction(instructions, "deposit");
        assert_eq!(deposit["arguments"][1]["name"], "args");
        assert_eq!(deposit["arguments"][1]["type"]["name"], "depositArgs");

        let process_withdrawals = instruction(instructions, "processWithdrawals");
        assert_eq!(
            process_withdrawals["arguments"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn instruction_decode_rejects_unknown_values() {
        assert!(RoshiInstruction::decode(&[255]).is_err());
    }

    #[test]
    fn serialize_instruction_writes_tag_then_args_payload() {
        let args = DepositArgs {
            asset_mint: [4; 32],
            amount: 123,
            min_shares_out: 456,
            access_proof: vec![[1; 32], [2; 32], [3; 32]],
        };

        let encoded = serialize_instruction(&args).unwrap();
        let decoded: DepositArgs = deserialize_exact(&encoded[1..]).unwrap();

        assert_eq!(encoded[0], <DepositArgs as InstructionArgs>::TAG);
        assert_eq!(decoded.asset_mint, [4; 32]);
        assert_eq!(decoded.amount, 123);
        assert_eq!(decoded.min_shares_out, 456);
        assert_eq!(decoded.access_proof, vec![[1; 32], [2; 32], [3; 32]]);
    }

    #[test]
    fn canonical_instruction_serializes_like_args_helper() {
        let args = DepositArgs {
            asset_mint: [4; 32],
            amount: 123,
            min_shares_out: 456,
            access_proof: vec![[1; 32]],
        };

        assert_eq!(
            RoshiInstruction::Deposit(args).serialize().unwrap(),
            serialize_instruction(&DepositArgs {
                asset_mint: [4; 32],
                amount: 123,
                min_shares_out: 456,
                access_proof: vec![[1; 32]],
            })
            .unwrap()
        );
    }

    #[test]
    fn serialize_zero_sized_args_writes_only_tag() {
        assert_eq!(
            serialize_instruction(&ProcessWithdrawalsArgs).unwrap(),
            vec![<ProcessWithdrawalsArgs as InstructionArgs>::TAG]
        );
        assert_eq!(
            RoshiInstruction::ProcessWithdrawals.serialize().unwrap(),
            vec![<ProcessWithdrawalsArgs as InstructionArgs>::TAG]
        );
    }

    fn instruction<'a>(instructions: &'a [Value], name: &str) -> &'a Value {
        instructions
            .iter()
            .find(|instruction| instruction["name"] == name)
            .unwrap()
    }

    fn assert_instruction_discriminator(instructions: &[Value], name: &str, tag: u8) {
        assert_eq!(
            instruction(instructions, name)["arguments"][0]["defaultValue"]["number"],
            u64::from(tag)
        );
    }

    const IDL_TAG_CASES: &[(&str, u8)] = &[
        ("initializeProgram", tags::INITIALIZE_PROGRAM),
        ("initializeVault", tags::INITIALIZE_VAULT),
        ("authorizeAction", tags::AUTHORIZE_ACTION),
        ("revokeAction", tags::REVOKE_ACTION),
        ("manage", tags::MANAGE),
        ("manageBatch", tags::MANAGE_BATCH),
        ("reportNav", tags::REPORT_NAV),
        ("deposit", tags::DEPOSIT),
        ("redeem", tags::REDEEM),
        ("cancelRedeem", tags::CANCEL_REDEEM),
        ("processWithdrawals", tags::PROCESS_WITHDRAWALS),
        ("updateVaultConfig", tags::UPDATE_VAULT_CONFIG),
        ("initializeAsset", tags::INITIALIZE_ASSET),
        ("updateAsset", tags::UPDATE_ASSET),
        ("setPauseFlags", tags::SET_PAUSE_FLAGS),
        ("setVaultAccess", tags::SET_VAULT_ACCESS),
        ("transferProgramAuthority", tags::TRANSFER_PROGRAM_AUTHORITY),
        ("transferVaultAuthority", tags::TRANSFER_VAULT_AUTHORITY),
        ("setStrategist", tags::SET_STRATEGIST),
        ("setNavAuthority", tags::SET_NAV_AUTHORITY),
        ("setWithdrawalAuthority", tags::SET_WITHDRAWAL_AUTHORITY),
        ("collectFees", tags::COLLECT_FEES),
        ("investExternal", tags::INVEST_EXTERNAL),
        ("returnExternal", tags::RETURN_EXTERNAL),
        ("setSwapAuthority", tags::SET_SWAP_AUTHORITY),
        ("atomicRedeem", tags::ATOMIC_REDEEM),
        ("swap", tags::SWAP),
    ];
}
