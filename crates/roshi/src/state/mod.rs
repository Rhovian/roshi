pub mod action;
pub mod asset;
pub mod program_config;
pub mod sub_account;
pub mod vault;
pub mod withdrawal_ticket;

use action::Action;
use asset::Asset;
use program_config::ProgramConfig;
use vault::Vault;
use wincode::{SchemaRead, SchemaWrite};
use withdrawal_ticket::WithdrawalTicket;

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
