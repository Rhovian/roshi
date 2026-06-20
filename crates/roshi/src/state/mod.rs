pub mod action;
pub mod asset;
pub mod external_destination;
pub mod program_config;
pub mod sub_account;
pub mod vault;
pub mod withdrawal_ticket;

use action::Action;
use asset::Asset;
use external_destination::ExternalDestination;
use program_config::ProgramConfig;
use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use vault::Vault;
use wincode::{deserialize, SchemaRead, SchemaWrite};
use withdrawal_ticket::WithdrawalTicket;

use roshi_interface::error::RoshiError;

/// Tagged account storage keeps owned deserialization for now.
///
/// The concrete fixed-size payload structs are zero-copy eligible, but the
/// one-byte enum tag means the payload would start at offset 1 in account
/// data. That can misalign 8-byte fields, so mutable zero-copy references need
/// a different discriminator layout or explicit alignment handling first.
#[derive(SchemaWrite, SchemaRead)]
#[wincode(tag_encoding = "u8")]
#[allow(clippy::large_enum_variant)]
pub enum Account {
    // Tag 0 is reserved and unmapped: a zeroed (freshly-allocated, uninitialized)
    // roshi-owned account must never deserialize to a valid typed account. The
    // wincode read path errors on an unmapped tag, so `Account::load` rejects it
    // rather than decoding it as an all-zero `ProgramConfig` — the invariant is
    // held by the encoding, not by every call site remembering a PDA check.
    #[wincode(tag = 1)]
    Vault(Vault),
    #[wincode(tag = 2)]
    Action(Action),
    #[wincode(tag = 3)]
    WithdrawalTicket(WithdrawalTicket),
    #[wincode(tag = 4)]
    Asset(Asset),
    #[wincode(tag = 5)]
    ExternalDestination(ExternalDestination),
    #[wincode(tag = 6)]
    ProgramConfig(ProgramConfig),
}

pub trait AccountData: Sized {
    fn try_from_account(account: Account) -> Result<Self, ProgramError>;

    fn validate(&self) -> ProgramResult {
        Ok(())
    }
}

impl Account {
    pub fn load(account: &AccountInfo) -> Result<Self, ProgramError> {
        if account.owner != &crate::ID {
            return Err(ProgramError::IllegalOwner);
        }

        let data = account.data.borrow();
        deserialize(&data).map_err(|_| ProgramError::InvalidAccountData)
    }

    pub fn load_as<T: AccountData>(account: &AccountInfo) -> Result<T, ProgramError> {
        let value = T::try_from_account(Self::load(account)?)?;
        value.validate()?;

        Ok(value)
    }
}

macro_rules! impl_account_data {
    ($account_type:ty, $variant:ident, $error:ident) => {
        impl AccountData for $account_type {
            fn try_from_account(account: Account) -> Result<Self, ProgramError> {
                match account {
                    Account::$variant(value) => Ok(value),
                    _ => Err(RoshiError::$error.into()),
                }
            }
        }
    };
}

impl_account_data!(ProgramConfig, ProgramConfig, InvalidProgramConfigAccount);
impl_account_data!(Action, Action, InvalidActionAccount);
impl_account_data!(
    ExternalDestination,
    ExternalDestination,
    InvalidExternalDestinationAccount
);
impl_account_data!(
    WithdrawalTicket,
    WithdrawalTicket,
    InvalidWithdrawalTicketAccount
);

impl AccountData for Vault {
    fn try_from_account(account: Account) -> Result<Self, ProgramError> {
        match account {
            Account::Vault(value) => Ok(value),
            _ => Err(RoshiError::InvalidVaultAccount.into()),
        }
    }

    fn validate(&self) -> ProgramResult {
        self.validate_state()
    }
}

impl AccountData for Asset {
    fn try_from_account(account: Account) -> Result<Self, ProgramError> {
        match account {
            Account::Asset(value) => Ok(value),
            _ => Err(RoshiError::InvalidAssetAccount.into()),
        }
    }

    fn validate(&self) -> ProgramResult {
        self.validate_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_pubkey::Pubkey;
    use wincode::serialize;

    #[test]
    fn zeroed_account_does_not_decode_to_a_valid_type() {
        // A freshly-allocated roshi-owned account is all zeros. Tag 0 is reserved
        // and unmapped, so it must fail to deserialize rather than decoding as a
        // valid all-zero `ProgramConfig` (the #17 footgun).
        let zeroed = [0u8; ProgramConfig::SPACE];
        assert!(deserialize::<Account>(&zeroed).is_err());

        // And through the on-chain entry point: a zeroed, roshi-owned account is
        // rejected as `InvalidAccountData`, not loaded as a `ProgramConfig`.
        let key = Pubkey::new_unique();
        let owner = crate::ID;
        let mut lamports = 1;
        let mut data = [0u8; ProgramConfig::SPACE];
        let account = AccountInfo::new(&key, false, false, &mut lamports, &mut data, &owner, false);
        assert_eq!(
            Account::load_as::<ProgramConfig>(&account),
            Err(ProgramError::InvalidAccountData)
        );
    }

    #[test]
    fn program_config_round_trips_through_its_reserved_tag() {
        // The retag (0 -> 6) still serializes and loads cleanly.
        let config = ProgramConfig::new(Pubkey::new_unique());
        let bytes = serialize(&Account::ProgramConfig(config)).unwrap();
        assert_eq!(bytes[0], 6, "ProgramConfig now lives at tag 6");
        match deserialize::<Account>(&bytes).unwrap() {
            Account::ProgramConfig(decoded) => assert_eq!(decoded, config),
            _ => panic!("expected a ProgramConfig"),
        }
    }

    #[test]
    fn tagged_account_payloads_are_not_loaded_by_zero_copy_yet() {
        assert_eq!(core::mem::align_of::<ProgramConfig>(), 1);
        assert_eq!(core::mem::align_of::<Vault>(), 8);
        assert_eq!(core::mem::align_of::<Asset>(), 8);
        assert_eq!(core::mem::align_of::<WithdrawalTicket>(), 8);

        let tag_len = core::mem::size_of::<u8>();
        assert_ne!(tag_len % core::mem::align_of::<Vault>(), 0);
        assert_ne!(tag_len % core::mem::align_of::<Asset>(), 0);
        assert_ne!(tag_len % core::mem::align_of::<WithdrawalTicket>(), 0);
    }
}
