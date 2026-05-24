use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct WithdrawalTicket {
    pub vault: [u8; 32],
    pub owner: [u8; 32],
    pub assets_owed: u64,
    pub epoch: u64,
    pub bump: u8,
}

impl WithdrawalTicket {
    pub const SEED: &'static [u8] = b"ticket";

    pub fn find_address(vault: &Pubkey, owner: &Pubkey, epoch: u64) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[
                Self::SEED,
                vault.as_ref(),
                owner.as_ref(),
                &epoch.to_le_bytes(),
            ],
            &crate::ID,
        )
    }
}
