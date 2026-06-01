pub mod args;

pub use args::*;

use wincode::{config::DefaultConfig, SchemaWrite};

pub trait InstructionArgs: SchemaWrite<DefaultConfig, Src = Self> {
    const TAG: RoshiInstructionTag;
}

macro_rules! roshi_instructions {
    ($( $variant:ident = $tag:literal => $args:ty ),+ $(,)?) => {
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum RoshiInstructionTag {
            $(
                $variant = $tag,
            )+
        }

        impl TryFrom<u8> for RoshiInstructionTag {
            type Error = ();

            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $(
                        $tag => Ok(Self::$variant),
                    )+
                    _ => Err(()),
                }
            }
        }

        impl From<RoshiInstructionTag> for u8 {
            fn from(tag: RoshiInstructionTag) -> Self {
                tag as u8
            }
        }

        $(
            impl InstructionArgs for $args {
                const TAG: RoshiInstructionTag = RoshiInstructionTag::$variant;
            }
        )+

        #[cfg(test)]
        const TAG_CASES: &[(u8, RoshiInstructionTag)] = &[
            $(
                ($tag, RoshiInstructionTag::$variant),
            )+
        ];
    };
}

roshi_instructions! {
    InitializeProgram = 0 => InitializeProgramArgs,
    InitializeVault = 1 => InitializeVaultArgs,
    AuthorizeAction = 2 => AuthorizeActionArgs,
    RevokeAction = 3 => RevokeActionArgs,
    Manage = 4 => ManageArgs,
    ManageBatch = 5 => ManageBatchArgs,
    Deposit = 7 => DepositArgs,
    Redeem = 8 => RedeemArgs,
    ProcessWithdrawals = 10 => ProcessWithdrawalsArgs,
    UpdateVaultConfig = 11 => UpdateVaultConfigArgs,
    InitializeAsset = 12 => InitializeAssetArgs,
    UpdateAsset = 13 => UpdateAssetArgs,
    // 14 was InitializeSubAccount: removed — subaccounts are bare PDA signer
    // seeds and need no on-chain initialization.
    SetPauseFlags = 15 => SetPauseFlagsArgs,
    SetVaultAccess = 16 => SetVaultAccessArgs,
    TransferProgramAuthority = 17 => TransferProgramAuthorityArgs,
    TransferVaultAuthority = 18 => TransferVaultAuthorityArgs,
    SetStrategist = 19 => SetStrategistArgs,
    SetNavAuthority = 20 => SetNavAuthorityArgs,
    SetWithdrawalAuthority = 21 => SetWithdrawalAuthorityArgs,
}

pub fn serialize_instruction<T>(args: &T) -> Result<Vec<u8>, wincode::WriteError>
where
    T: InstructionArgs,
{
    let mut data = vec![u8::from(T::TAG)];
    wincode::serialize_into(&mut data, args)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincode::deserialize_exact;

    #[test]
    fn instruction_tags_round_trip_from_wire_byte() {
        for (wire_byte, tag) in TAG_CASES {
            assert_eq!(u8::from(*tag), *wire_byte);
            assert_eq!(RoshiInstructionTag::try_from(*wire_byte), Ok(*tag));
        }
    }

    #[test]
    fn instruction_tag_rejects_unknown_values() {
        assert_eq!(RoshiInstructionTag::try_from(6), Err(()));
        assert_eq!(RoshiInstructionTag::try_from(9), Err(()));
        assert_eq!(RoshiInstructionTag::try_from(255), Err(()));
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

        assert_eq!(encoded[0], u8::from(RoshiInstructionTag::Deposit));
        assert_eq!(decoded.asset_mint, [4; 32]);
        assert_eq!(decoded.amount, 123);
        assert_eq!(decoded.min_shares_out, 456);
        assert_eq!(decoded.access_proof, vec![[1; 32], [2; 32], [3; 32]]);
    }

    #[test]
    fn serialize_zero_sized_args_writes_only_tag() {
        assert_eq!(
            serialize_instruction(&ProcessWithdrawalsArgs).unwrap(),
            vec![u8::from(RoshiInstructionTag::ProcessWithdrawals)]
        );
    }
}
