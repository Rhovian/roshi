pub mod args;

pub use args::{
    IndexedActionArgs, InitializeAssetArgs, InitializeVaultArgs, SetNavAuthorityArgs,
    SetPauseFlagsArgs, SetStrategistArgs, SetVaultAccessArgs, SetWithdrawalAuthorityArgs,
    UpdateAssetArgs, UpdateVaultConfigArgs,
};

use crate::action::Ops;
use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead)]
#[wincode(tag_encoding = "u8")]
pub enum RoshiInstruction {
    #[wincode(tag = 0)]
    InitializeProgram { authority: [u8; 32] },
    #[wincode(tag = 1)]
    InitializeVault { args: InitializeVaultArgs },
    #[wincode(tag = 2)]
    AuthorizeAction { action_hash: [u8; 32], ops: Ops },
    #[wincode(tag = 3)]
    RevokeAction { action_hash: [u8; 32] },
    #[wincode(tag = 4)]
    Manage {
        sub_account: u8,
        program_id: [u8; 32],
        accounts_start: u8,
        accounts_len: u8,
        ix_data: Vec<u8>,
    },
    #[wincode(tag = 5)]
    ManageBatch { actions: Vec<IndexedActionArgs> },
    #[wincode(tag = 7)]
    Deposit {
        asset_mint: [u8; 32],
        amount: u64,
        min_shares_out: u64,
        access_proof: Vec<[u8; 32]>,
    },
    #[wincode(tag = 8)]
    Redeem {
        ticket_index: u8,
        shares: u64,
        min_assets_out: u64,
    },
    #[wincode(tag = 10)]
    ProcessWithdrawals,
    #[wincode(tag = 11)]
    UpdateVaultConfig { args: UpdateVaultConfigArgs },
    #[wincode(tag = 12)]
    InitializeAsset { args: InitializeAssetArgs },
    #[wincode(tag = 13)]
    UpdateAsset { args: UpdateAssetArgs },
    #[wincode(tag = 14)]
    InitializeSubAccount { index: u8 },
    #[wincode(tag = 15)]
    SetPauseFlags { args: SetPauseFlagsArgs },
    #[wincode(tag = 16)]
    SetVaultAccess { args: SetVaultAccessArgs },
    #[wincode(tag = 17)]
    TransferProgramAuthority { new_authority: [u8; 32] },
    #[wincode(tag = 18)]
    TransferVaultAuthority { new_authority: [u8; 32] },
    #[wincode(tag = 19)]
    SetStrategist { args: SetStrategistArgs },
    #[wincode(tag = 20)]
    SetNavAuthority { args: SetNavAuthorityArgs },
    #[wincode(tag = 21)]
    SetWithdrawalAuthority { args: SetWithdrawalAuthorityArgs },
}

#[cfg(test)]
mod tests {
    use super::*;
    use wincode::{deserialize, serialize};

    #[test]
    fn deposit_round_trips_with_access_proof() {
        let proof = vec![[1; 32], [2; 32], [3; 32]];
        let encoded = serialize(&RoshiInstruction::Deposit {
            asset_mint: [4; 32],
            amount: 123,
            min_shares_out: 456,
            access_proof: proof.clone(),
        })
        .unwrap();

        let decoded = deserialize(&encoded).unwrap();

        match decoded {
            RoshiInstruction::Deposit {
                asset_mint,
                amount,
                min_shares_out,
                access_proof,
            } => {
                assert_eq!(asset_mint, [4; 32]);
                assert_eq!(amount, 123);
                assert_eq!(min_shares_out, 456);
                assert_eq!(access_proof, proof);
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn set_vault_access_round_trips() {
        let encoded = serialize(&RoshiInstruction::SetVaultAccess {
            args: SetVaultAccessArgs {
                private: true,
                access_merkle_root: [9; 32],
            },
        })
        .unwrap();

        let decoded = deserialize(&encoded).unwrap();

        match decoded {
            RoshiInstruction::SetVaultAccess { args } => {
                assert!(args.private);
                assert_eq!(args.access_merkle_root, [9; 32]);
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn authority_transfer_instructions_round_trip() {
        let encoded = serialize(&RoshiInstruction::TransferProgramAuthority {
            new_authority: [3; 32],
        })
        .unwrap();

        match deserialize(&encoded).unwrap() {
            RoshiInstruction::TransferProgramAuthority { new_authority } => {
                assert_eq!(new_authority, [3; 32]);
            }
            _ => panic!("unexpected instruction"),
        }

        let encoded = serialize(&RoshiInstruction::TransferVaultAuthority {
            new_authority: [4; 32],
        })
        .unwrap();

        match deserialize(&encoded).unwrap() {
            RoshiInstruction::TransferVaultAuthority { new_authority } => {
                assert_eq!(new_authority, [4; 32]);
            }
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn rbac_setter_instructions_round_trip() {
        let encoded = serialize(&RoshiInstruction::SetStrategist {
            args: SetStrategistArgs {
                strategist: [1; 32],
            },
        })
        .unwrap();
        match deserialize(&encoded).unwrap() {
            RoshiInstruction::SetStrategist { args } => assert_eq!(args.strategist, [1; 32]),
            _ => panic!("unexpected instruction"),
        }

        let encoded = serialize(&RoshiInstruction::SetNavAuthority {
            args: SetNavAuthorityArgs {
                nav_authority: [2; 32],
            },
        })
        .unwrap();
        match deserialize(&encoded).unwrap() {
            RoshiInstruction::SetNavAuthority { args } => {
                assert_eq!(args.nav_authority, [2; 32]);
            }
            _ => panic!("unexpected instruction"),
        }

        let encoded = serialize(&RoshiInstruction::SetWithdrawalAuthority {
            args: SetWithdrawalAuthorityArgs {
                withdrawal_authority: [3; 32],
            },
        })
        .unwrap();
        match deserialize(&encoded).unwrap() {
            RoshiInstruction::SetWithdrawalAuthority { args } => {
                assert_eq!(args.withdrawal_authority, [3; 32]);
            }
            _ => panic!("unexpected instruction"),
        }
    }
}
