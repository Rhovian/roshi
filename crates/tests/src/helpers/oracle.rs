use litesvm::LiteSVM;
use roshi::oracle::scope::{
    DATED_PRICES_OFFSET, DATED_PRICE_SIZE, GENERIC_OFFSET, ORACLE_MAPPINGS_DISCRIMINATOR,
    ORACLE_MAPPINGS_LEN, ORACLE_PRICES_DISCRIMINATOR, ORACLE_PRICES_LEN,
    ORACLE_PRICES_MAPPINGS_OFFSET, PRICE_INFO_ACCOUNTS_OFFSET, PRICE_TYPES_OFFSET,
    REF_PRICE_OFFSET, SCOPE_PROGRAM_ID, TWAP_ENABLED_BITMASK_OFFSET,
    TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS_OFFSET,
};
use roshi::oracle::ScopeOracleConfig;
use solana_pubkey::Pubkey;
use solana_sdk::account::Account;

/// Pyth Solana Receiver program id (owner of `PriceUpdateV2` accounts).
pub const PYTH_RECEIVER_ID: Pubkey =
    solana_pubkey::pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

/// Install the mock Kamino Scope `OraclePrices` + `OracleMappings` account
/// pair a config points at, holding one unfrozen entry with the given
/// observation. The account shape comes from the reader's layout constants,
/// the addresses and complete selected mapping from the config — except the
/// mappings address, which by design lives in the prices account, not the
/// config.
pub fn set_scope_oracle(
    svm: &mut LiteSVM,
    config: &ScopeOracleConfig,
    mappings_account: Pubkey,
    value: u64,
    exp: u64,
    unix_timestamp: u64,
) {
    let prices_account = Pubkey::new_from_array(config.prices_account);
    let index = usize::from(config.price_index);

    let mut prices = vec![0u8; ORACLE_PRICES_LEN];
    prices[..8].copy_from_slice(ORACLE_PRICES_DISCRIMINATOR);
    prices[ORACLE_PRICES_MAPPINGS_OFFSET..ORACLE_PRICES_MAPPINGS_OFFSET + 32]
        .copy_from_slice(&mappings_account.to_bytes());
    let base = DATED_PRICES_OFFSET + DATED_PRICE_SIZE * index;
    prices[base..base + 8].copy_from_slice(&value.to_le_bytes());
    prices[base + 8..base + 16].copy_from_slice(&exp.to_le_bytes());
    prices[base + 24..base + 32].copy_from_slice(&unix_timestamp.to_le_bytes());

    let mut mappings = vec![0u8; ORACLE_MAPPINGS_LEN];
    mappings[..8].copy_from_slice(ORACLE_MAPPINGS_DISCRIMINATOR);
    let info = PRICE_INFO_ACCOUNTS_OFFSET + 32 * index;
    mappings[info..info + 32].copy_from_slice(&config.mapping.price_info_account);
    mappings[PRICE_TYPES_OFFSET + index] = config.mapping.price_type;
    let twap_source_or_ref_price_tolerance =
        TWAP_SOURCE_OR_REF_PRICE_TOLERANCE_BPS_OFFSET + 2 * index;
    mappings[twap_source_or_ref_price_tolerance..twap_source_or_ref_price_tolerance + 2]
        .copy_from_slice(
            &config
                .mapping
                .twap_source_or_ref_price_tolerance_bps
                .to_le_bytes(),
        );
    mappings[TWAP_ENABLED_BITMASK_OFFSET + index] = config.mapping.twap_enabled_bitmask;
    let ref_price = REF_PRICE_OFFSET + 2 * index;
    mappings[ref_price..ref_price + 2].copy_from_slice(&config.mapping.ref_price.to_le_bytes());
    let generic = GENERIC_OFFSET + 20 * index;
    mappings[generic..generic + 20].copy_from_slice(&config.mapping.generic);

    for (address, data) in [(prices_account, prices), (mappings_account, mappings)] {
        let lamports = svm.minimum_balance_for_rent_exemption(data.len());
        svm.set_account(
            address,
            Account {
                lamports,
                data,
                owner: SCOPE_PROGRAM_ID,
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
