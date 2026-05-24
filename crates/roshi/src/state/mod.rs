pub mod action;
pub mod program_config;
pub mod vault;
pub mod withdrawal_ticket;

use action::Action;
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
}
