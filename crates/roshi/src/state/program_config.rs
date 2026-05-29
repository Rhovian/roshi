use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

use crate::state::Account;

#[derive(SchemaWrite, SchemaRead)]
#[repr(C)]
pub struct ProgramConfig {
    authority: [u8; 32],
}

impl ProgramConfig {
    pub const SEED: &'static [u8] = b"program_config";
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    pub fn new(authority: Pubkey) -> Self {
        Self {
            authority: authority.to_bytes(),
        }
    }

    pub fn find_address() -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED], &crate::ID)
    }

    pub fn authority(&self) -> Pubkey {
        Pubkey::from(self.authority)
    }

    pub fn verify_authority(config_acc: &AccountInfo, signer: &AccountInfo) -> ProgramResult {
        let (expected_config_key, _) = Self::find_address();
        if config_acc.key != &expected_config_key {
            return Err(ProgramError::InvalidSeeds);
        }

        let config = Account::load_as::<ProgramConfig>(config_acc)?;

        if signer.key != &config.authority() {
            return Err(ProgramError::IllegalOwner);
        }

        if !signer.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        Ok(())
    }
}
