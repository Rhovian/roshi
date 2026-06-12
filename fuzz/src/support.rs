//! Low-level SPL account installation + reads, ported from
//! `roshi-tests/src/helpers/token.rs`. The fuzzer drives the program through
//! real instructions, but custody/user token state is installed directly so
//! `setup()` is fast and deterministic. Operates on the `LiteSVM` behind
//! `TestContext`, so call these with `&mut ctx.svm`.

use crucible_test_context::litesvm::LiteSVM;
use solana_account::Account;
use solana_pubkey::Pubkey;

/// SPL Token program id (classic).
pub const TOKEN_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
/// Token-2022 program id.
pub const TOKEN_2022_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
/// Associated Token Account program id.
pub const ATA_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Derive the associated token account address for `(wallet, mint)` under the
/// classic SPL Token program.
pub fn associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    associated_token_address_with_program(wallet, mint, &TOKEN_PROGRAM_ID)
}

/// Derive the associated token account address for `(wallet, mint)` under the
/// supplied SPL Token program.
pub fn associated_token_address_with_program(
    wallet: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ATA_PROGRAM_ID,
    )
    .0
}

/// Install an initialized SPL mint with `authority` as the mint authority.
pub fn set_mint(svm: &mut LiteSVM, mint: Pubkey, authority: &Pubkey, decimals: u8) {
    set_mint_with_program(svm, mint, authority, decimals, TOKEN_PROGRAM_ID);
}

/// Install an initialized bare Token-2022 mint with no extensions. The program
/// deliberately accepts this 82-byte shape and rejects extended mints.
pub fn set_token_2022_mint(svm: &mut LiteSVM, mint: Pubkey, authority: &Pubkey, decimals: u8) {
    set_mint_with_program(svm, mint, authority, decimals, TOKEN_2022_PROGRAM_ID);
}

/// Install a Token-2022 mint carrying only the allowlisted metadata-pointer
/// extension — mint verification must accept it.
pub fn set_metadata_pointer_token_2022_mint(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
) {
    set_token_2022_mint_with_extension(svm, mint, authority, decimals, 18, 64);
}

/// Install a Token-2022 mint carrying a transfer-fee extension — outside the
/// allowlist, mint verification must reject it.
pub fn set_transfer_fee_token_2022_mint(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
) {
    set_token_2022_mint_with_extension(svm, mint, authority, decimals, 1, 108);
}

/// Base mint layout padded to the token-account length, the mint
/// account-type byte, then one `(type, len, zeroed value)` TLV entry.
fn set_token_2022_mint_with_extension(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
    extension_type: u16,
    extension_len: usize,
) {
    let len = 166 + 4 + extension_len;
    let mut data = vec![0u8; len];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..36].copy_from_slice(authority.as_ref());
    data[44] = decimals;
    data[45] = 1;
    data[165] = 1; // AccountType::Mint
    data[166..168].copy_from_slice(&extension_type.to_le_bytes());
    data[168..170].copy_from_slice(&(extension_len as u16).to_le_bytes());
    let lamports = svm.minimum_balance_for_rent_exemption(len);
    svm.set_account(
        mint,
        Account {
            lamports,
            data,
            owner: TOKEN_2022_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn set_mint_with_program(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
    token_program: Pubkey,
) {
    set_mint_with_len(svm, mint, authority, decimals, token_program, 82);
}

fn set_mint_with_len(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
    token_program: Pubkey,
    len: usize,
) {
    let mut data = vec![0u8; 82];
    data.resize(len, 0);
    data[0..4].copy_from_slice(&1u32.to_le_bytes()); // mint_authority COption::Some
    data[4..36].copy_from_slice(authority.as_ref());
    data[44] = decimals;
    data[45] = 1; // is_initialized
    let lamports = svm.minimum_balance_for_rent_exemption(len);
    svm.set_account(
        mint,
        Account {
            lamports,
            data,
            owner: token_program,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

/// Install an initialized SPL token account holding `amount` of `mint`.
pub fn set_token_account(
    svm: &mut LiteSVM,
    address: Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
) {
    set_token_account_with_program(svm, address, mint, owner, amount, TOKEN_PROGRAM_ID);
}

/// Install an initialized token account holding `amount` of `mint` and owned by
/// the supplied SPL Token program.
pub fn set_token_account_with_program(
    svm: &mut LiteSVM,
    address: Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    token_program: Pubkey,
) {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1; // AccountState::Initialized
    let lamports = svm.minimum_balance_for_rent_exemption(165);
    svm.set_account(
        address,
        Account {
            lamports,
            data,
            owner: token_program,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

/// Install + fund the ATA for `(owner, mint)`; returns its address.
pub fn set_ata(svm: &mut LiteSVM, owner: &Pubkey, mint: &Pubkey, amount: u64) -> Pubkey {
    let address = associated_token_address(owner, mint);
    set_token_account(svm, address, mint, owner, amount);
    address
}

/// Install + fund the ATA for `(owner, mint)` under the supplied token program.
pub fn set_ata_with_program(
    svm: &mut LiteSVM,
    owner: &Pubkey,
    mint: &Pubkey,
    amount: u64,
    token_program: Pubkey,
) -> Pubkey {
    let address = associated_token_address_with_program(owner, mint, &token_program);
    set_token_account_with_program(svm, address, mint, owner, amount, token_program);
    address
}

/// Pyth Solana Receiver program id (owner of `PriceUpdateV2` accounts).
pub const PYTH_RECEIVER_ID: Pubkey =
    solana_pubkey::pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

/// Build a mock fully-verified Pyth `PriceUpdateV2` payload, matching the
/// 133-byte layout the program parses. Ported from
/// `roshi-tests/src/helpers/oracle.rs::set_pyth_price`, generalized with a
/// `conf` parameter so the fuzzer can drive the confidence-width check.
pub fn pyth_price_data(
    feed_id: [u8; 32],
    price: i64,
    conf: u64,
    exponent: i32,
    publish_time: i64,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(133);
    data.extend_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]); // discriminator
    data.extend_from_slice(&[0u8; 32]); // write_authority
    data.push(1); // VerificationLevel::Full
    data.extend_from_slice(&feed_id);
    data.extend_from_slice(&price.to_le_bytes());
    data.extend_from_slice(&conf.to_le_bytes());
    data.extend_from_slice(&exponent.to_le_bytes());
    data.extend_from_slice(&publish_time.to_le_bytes());
    data.extend_from_slice(&publish_time.to_le_bytes()); // prev_publish_time
    data.extend_from_slice(&0i64.to_le_bytes()); // ema_price
    data.extend_from_slice(&0u64.to_le_bytes()); // ema_conf
    data.extend_from_slice(&0u64.to_le_bytes()); // posted_slot
    data
}

/// Install a mock Pyth `PriceUpdateV2` account owned by the Pyth receiver
/// program (the owner check is on the receiver, not the token program).
/// Setup-time only: mid-action installs must go through `ctx.write_account`
/// so the per-iteration snapshot restore sees the mutation.
pub fn set_pyth_price(
    svm: &mut LiteSVM,
    address: Pubkey,
    feed_id: [u8; 32],
    price: i64,
    conf: u64,
    exponent: i32,
    publish_time: i64,
) {
    let data = pyth_price_data(feed_id, price, conf, exponent, publish_time);
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

/// Read the `amount` field of an SPL token account. Every account the harness
/// reads is installed in `setup()`, so a missing/short account is a harness bug,
/// not a 0 balance — fail loudly rather than silently masking it.
pub fn token_balance(svm: &LiteSVM, address: &Pubkey) -> u64 {
    let account = svm
        .get_account(address)
        .unwrap_or_else(|| panic!("token account {address} missing"));
    let bytes: [u8; 8] = account
        .data
        .get(64..72)
        .unwrap_or_else(|| {
            panic!(
                "token account {address} too short ({}B)",
                account.data.len()
            )
        })
        .try_into()
        .unwrap();
    u64::from_le_bytes(bytes)
}

/// Read the `supply` field of an SPL mint. Fails loudly on a missing/short
/// account for the same reason as [`token_balance`].
pub fn mint_supply(svm: &LiteSVM, mint: &Pubkey) -> u64 {
    let account = svm
        .get_account(mint)
        .unwrap_or_else(|| panic!("mint {mint} missing"));
    let bytes: [u8; 8] = account
        .data
        .get(36..44)
        .unwrap_or_else(|| panic!("mint {mint} too short ({}B)", account.data.len()))
        .try_into()
        .unwrap();
    u64::from_le_bytes(bytes)
}
