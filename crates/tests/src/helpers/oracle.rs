use litesvm::LiteSVM;
use solana_pubkey::Pubkey;
use solana_sdk::account::Account;

/// Pyth Solana Receiver program id (owner of `PriceUpdateV2` accounts).
pub const PYTH_RECEIVER_ID: Pubkey =
    solana_pubkey::pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

/// Install a mock fully-verified Pyth `PriceUpdateV2` account (zero confidence)
/// owned by the Pyth receiver program, matching the layout the program parses.
pub fn set_pyth_price(
    svm: &mut LiteSVM,
    address: Pubkey,
    feed_id: [u8; 32],
    price: i64,
    exponent: i32,
    publish_time: i64,
) {
    let mut data = Vec::with_capacity(133);
    data.extend_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]); // discriminator
    data.extend_from_slice(&[0u8; 32]); // write_authority
    data.push(1); // VerificationLevel::Full
    data.extend_from_slice(&feed_id);
    data.extend_from_slice(&price.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes()); // conf
    data.extend_from_slice(&exponent.to_le_bytes());
    data.extend_from_slice(&publish_time.to_le_bytes());
    data.extend_from_slice(&publish_time.to_le_bytes()); // prev_publish_time
    data.extend_from_slice(&0i64.to_le_bytes()); // ema_price
    data.extend_from_slice(&0u64.to_le_bytes()); // ema_conf
    data.extend_from_slice(&0u64.to_le_bytes()); // posted_slot
    let lamports = svm.minimum_balance_for_rent_exemption(data.len());
    svm.set_account(
        address,
        Account {
            lamports,
            data,
            owner: PYTH_RECEIVER_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}
