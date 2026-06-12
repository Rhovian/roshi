use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

/// Admin-registered destination for `invest_external`: the only token
/// accounts the strategist may move custody to. Mirrors the Asset/action-hash
/// philosophy — the admin authorizes venues, the strategist only moves funds
/// between custody and authorized venues.
#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct ExternalDestination {
    /// Vault this destination is registered for.
    pub vault: [u8; 32],
    /// The authorized destination token account.
    pub token_account: [u8; 32],
    pub bump: u8,
}

impl ExternalDestination {
    pub const SEED: &'static [u8] = b"external_destination";
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    pub fn new(vault: [u8; 32], token_account: [u8; 32], bump: u8) -> Self {
        Self {
            vault,
            token_account,
            bump,
        }
    }

    pub fn find_address(vault: &Pubkey, token_account: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED, vault.as_ref(), token_account.as_ref()],
            &crate::ID,
        )
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
    fn external_destination_is_zero_copy_without_padding() {
        let destination = ExternalDestination::new([1; 32], [2; 32], 3);

        assert_zero_copy::<ExternalDestination>();
        assert_eq!(core::mem::size_of::<ExternalDestination>(), 65);
        assert_eq!(ExternalDestination::SPACE, 66);
        assert_eq!(
            serialize(&destination).unwrap().len(),
            core::mem::size_of::<ExternalDestination>()
        );
    }
}
