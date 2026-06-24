//! The `Asset` account lives in `roshi-interface` (the off-chain wire type); re-exported
//! here so the program keeps `crate::state::Asset`. Below: the `Account`-tagged load path
//! tests, which exercise the program's own dispatch.

pub use roshi_interface::state::Asset;

#[cfg(test)]
mod tests {
    use roshi_interface::error::RoshiError;
    use roshi_interface::oracle::OracleConfig;
    use solana_account_info::AccountInfo;
    use solana_program_error::ProgramError;
    use solana_pubkey::Pubkey;
    use wincode::serialize;

    use super::Asset;
    use crate::state::Account;

    fn test_asset() -> Asset {
        Asset::new(
            [1; 32],
            [2; 32],
            OracleConfig::default(),
            6,
            true,
            false,
            u64::MAX,
            7,
        )
        .unwrap()
    }

    /// Offset of `enabled_flag` in a serialized `Account::Asset` — the account tag byte,
    /// the two pubkeys, the oracle, the cap, then the decimals. `routed_flag` follows.
    fn enabled_flag_offset() -> usize {
        1 + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<OracleConfig>()
            + core::mem::size_of::<u64>()
            + core::mem::size_of::<u8>()
    }

    /// `Account::load_as::<Asset>` over `data`, asserting it rejects with the asset error.
    fn assert_rejected(mut data: Vec<u8>) {
        let key = Pubkey::new_unique();
        let owner = crate::ID;
        let mut lamports = 1;
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);
        assert_eq!(
            Account::load_as::<Asset>(&account),
            Err(ProgramError::from(RoshiError::InvalidAssetAccount))
        );
    }

    #[test]
    fn load_as_rejects_invalid_enabled_flag() {
        let mut data = serialize(&Account::Asset(test_asset())).unwrap();
        data[enabled_flag_offset()] = 255;
        assert_rejected(data);
    }

    #[test]
    fn load_as_rejects_invalid_routed_flag() {
        let mut data = serialize(&Account::Asset(test_asset())).unwrap();
        data[enabled_flag_offset() + 1] = 255;
        assert_rejected(data);
    }

    #[test]
    fn load_as_rejects_invalid_oracle_kind() {
        let mut data = serialize(&Account::Asset(test_asset())).unwrap();
        let oracle_kind_offset = 1
            + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<[u8; 32]>()
            + core::mem::size_of::<roshi_interface::oracle::SwitchboardOracleConfig>()
            + core::mem::size_of::<roshi_interface::oracle::PythOracleConfig>();
        data[oracle_kind_offset] = 255;
        assert_rejected(data);
    }
}
