use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

pub const WITHDRAWAL_TICKET_COUNT: u16 = 256;

#[derive(SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct WithdrawalTicket {
    pub vault: [u8; 32],
    pub owner: [u8; 32],
    pub ticket_index: u8,
    pub request_epoch: u64,
    pub shares_burned: u64,
    pub assets_owed: u64,
    pub bump: u8,
}

impl WithdrawalTicket {
    pub const SEED: &'static [u8] = b"ticket";

    pub fn find_address(vault: &Pubkey, owner: &Pubkey, ticket_index: u8) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED, vault.as_ref(), owner.as_ref(), &[ticket_index]],
            &crate::ID,
        )
    }
}
