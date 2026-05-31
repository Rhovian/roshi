use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

use crate::state::Account;

#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
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

    pub fn verify_address(config_acc: &AccountInfo) -> Result<u8, ProgramError> {
        let (expected_config_key, config_bump) = Self::find_address();
        if config_acc.key != &expected_config_key {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(config_bump)
    }

    pub fn authority(&self) -> Pubkey {
        Pubkey::from(self.authority)
    }

    pub fn set_authority(&mut self, authority: Pubkey) {
        self.authority = authority.to_bytes();
    }

    pub fn verify_authority(config_acc: &AccountInfo, signer: &AccountInfo) -> ProgramResult {
        Self::verify_address(config_acc)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use wincode::{config::DefaultConfig, serialize, SchemaRead, SchemaWrite, TypeMeta};

    fn assert_zero_copy<T>()
    where
        T: wincode::ZeroCopy,
        T: for<'de> SchemaRead<'de, DefaultConfig> + SchemaWrite<DefaultConfig>,
    {
        assert_eq!(
            <T as SchemaRead<'_, DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
        assert_eq!(
            <T as SchemaWrite<DefaultConfig>>::TYPE_META,
            TypeMeta::Static {
                size: core::mem::size_of::<T>(),
                zero_copy: true,
            }
        );
    }

    #[test]
    fn program_config_is_zero_copy_without_padding() {
        let config = ProgramConfig::new(Pubkey::new_unique());

        assert_zero_copy::<ProgramConfig>();
        assert_eq!(core::mem::size_of::<ProgramConfig>(), 32);
        assert_eq!(ProgramConfig::SPACE, 33);
        assert_eq!(
            serialize(&config).unwrap().len(),
            core::mem::size_of::<ProgramConfig>()
        );
    }
}
