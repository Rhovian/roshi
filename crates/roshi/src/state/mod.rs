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
    #[wincode(tag = 0)]
    ProgramConfig(ProgramConfig),
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
