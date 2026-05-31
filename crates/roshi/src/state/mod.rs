pub mod action;
pub mod asset;
pub mod program_config;
pub mod sub_account;
pub mod vault;
pub mod withdrawal_ticket;

use action::Action;
use asset::Asset;
use program_config::ProgramConfig;
use solana_account_info::AccountInfo;
use solana_program_error::ProgramError;
use vault::Vault;
use wincode::{deserialize, SchemaRead, SchemaWrite};
use withdrawal_ticket::WithdrawalTicket;

use roshi_interface::error::RoshiError;

#[derive(SchemaWrite, SchemaRead)]
#[wincode(tag_encoding = "u8")]
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
}

pub trait AccountData: Sized {
    fn try_from_account(account: Account) -> Result<Self, ProgramError>;
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
        T::try_from_account(Self::load(account)?)
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
impl_account_data!(Vault, Vault, InvalidVaultAccount);
impl_account_data!(Action, Action, InvalidActionAccount);
impl_account_data!(
    WithdrawalTicket,
    WithdrawalTicket,
    InvalidWithdrawalTicketAccount
);
impl_account_data!(Asset, Asset, InvalidAssetAccount);
