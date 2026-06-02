use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

pub const WITHDRAWAL_TICKET_COUNT: u16 = 256;
pub const REDEEM_CANCEL_DELAY_SLOTS: u64 = 150;
pub const WITHDRAWAL_STRIKE_DELAY_EPOCHS: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
#[repr(C)]
pub struct WithdrawalTicket {
    pub vault: [u8; 32],
    pub owner: [u8; 32],
    pub recipient_token_account: [u8; 32],
    pub shares_burned: u64,
    pub assets_owed: u64,
    pub request_epoch: u64,
    pub request_slot: u64,
    pub ticket_index: u8,
    pub bump: u8,
    _padding: [u8; 6],
}

impl WithdrawalTicket {
    pub const SEED: &'static [u8] = b"ticket";
    pub const SPACE: usize = std::mem::size_of::<Self>() + 1;

    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        vault: [u8; 32],
        owner: [u8; 32],
        recipient_token_account: [u8; 32],
        ticket_index: u8,
        shares_burned: u64,
        assets_owed: u64,
        request_epoch: u64,
        request_slot: u64,
        bump: u8,
    ) -> Self {
        Self {
            vault,
            owner,
            recipient_token_account,
            shares_burned,
            assets_owed,
            request_epoch,
            request_slot,
            ticket_index,
            bump,
            _padding: [0; 6],
        }
    }

    pub fn find_address(
        vault: &Pubkey,
        recipient_token_account: &Pubkey,
        ticket_index: u8,
    ) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[
                Self::SEED,
                vault.as_ref(),
                recipient_token_account.as_ref(),
                &[ticket_index],
            ],
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
    fn withdrawal_ticket_is_zero_copy_with_explicit_padding() {
        let ticket = WithdrawalTicket::new([1; 32], [2; 32], [3; 32], 4, 5, 6, 7, 8, 9);

        assert_zero_copy::<WithdrawalTicket>();
        assert_eq!(core::mem::size_of::<WithdrawalTicket>(), 136);
        assert_eq!(WithdrawalTicket::SPACE, 137);
        assert_eq!(
            serialize(&ticket).unwrap().len(),
            core::mem::size_of::<WithdrawalTicket>()
        );
    }
}
