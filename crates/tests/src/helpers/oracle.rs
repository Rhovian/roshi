use litesvm::LiteSVM;
use solana_pubkey::Pubkey;
use solana_sdk::account::Account;

/// Pyth Solana Receiver program id (owner of `PriceUpdateV2` accounts).
pub const PYTH_RECEIVER_ID: Pubkey =
    solana_pubkey::pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

/// Install a mock Kamino Scope `OraclePrices` + `OracleMappings` account pair
/// holding one unfrozen entry, matching the layout the program reads: prices =
/// 8-byte discriminator, mappings pubkey, 512 56-byte `DatedPrice` entries;
/// mappings = 8-byte discriminator, 512 price-info pubkeys, then 512
/// price-type bytes.
#[allow(clippy::too_many_arguments)]
pub fn set_scope_oracle(
    svm: &mut LiteSVM,
    scope_program: Pubkey,
    prices_account: Pubkey,
    mappings_account: Pubkey,
    price_index: u16,
    price_info_account: [u8; 32],
    price_type: u8,
    value: u64,
    exp: u64,
    unix_timestamp: u64,
) {
    let index = usize::from(price_index);

    let mut prices = vec![0u8; 28_712];
    prices[..8].copy_from_slice(&[89, 128, 118, 221, 6, 72, 180, 146]);
    prices[8..40].copy_from_slice(&mappings_account.to_bytes());
    let base = 40 + 56 * index;
    prices[base..base + 8].copy_from_slice(&value.to_le_bytes());
    prices[base + 8..base + 16].copy_from_slice(&exp.to_le_bytes());
    prices[base + 24..base + 32].copy_from_slice(&unix_timestamp.to_le_bytes());

    let mut mappings = vec![0u8; 29_704];
    mappings[..8].copy_from_slice(&[40, 244, 110, 80, 255, 214, 243, 188]);
    mappings[8 + 32 * index..8 + 32 * index + 32].copy_from_slice(&price_info_account);
    mappings[16_392 + index] = price_type;

    for (address, data) in [(prices_account, prices), (mappings_account, mappings)] {
        let lamports = svm.minimum_balance_for_rent_exemption(data.len());
        svm.set_account(
            address,
            Account {
                lamports,
                data,
                owner: scope_program,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }
}

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
