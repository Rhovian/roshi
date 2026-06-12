use litesvm::LiteSVM;
use solana_pubkey::Pubkey;
use solana_sdk::account::Account;

/// SPL Token program id (classic).
pub const TOKEN_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
/// Token-2022 program id.
pub const TOKEN_2022_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
/// Associated Token Account program id.
pub const ATA_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Derive the associated token account address for `(wallet, mint)`.
pub fn associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    associated_token_address_with_program(wallet, mint, &TOKEN_PROGRAM_ID)
}

/// Derive the associated token account address for `(wallet, mint, token_program)`.
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

/// Install an initialized mint with `authority` as the mint authority.
pub fn set_mint_with_program(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
    token_program: Pubkey,
) {
    let mut data = vec![0u8; 82];
    data[0..4].copy_from_slice(&1u32.to_le_bytes()); // mint_authority COption::Some
    data[4..36].copy_from_slice(authority.as_ref());
    data[44] = decimals;
    data[45] = 1; // is_initialized
    let lamports = svm.minimum_balance_for_rent_exemption(82);
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

/// Install an initialized bare Token-2022 mint with no extensions.
pub fn set_token_2022_mint(svm: &mut LiteSVM, mint: Pubkey, authority: &Pubkey, decimals: u8) {
    set_mint_with_program(svm, mint, authority, decimals, TOKEN_2022_PROGRAM_ID);
}

/// Install an initialized extended Token-2022 mint that must be rejected.
const EXTENSION_TRANSFER_FEE_CONFIG: u16 = 1;
const EXTENSION_METADATA_POINTER: u16 = 18;

/// Token-2022 mint carrying only the allowlisted metadata-pointer extension.
pub fn set_metadata_pointer_token_2022_mint(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
) {
    set_token_2022_mint_with_extension(
        svm,
        mint,
        authority,
        decimals,
        EXTENSION_METADATA_POINTER,
        64,
    );
}

/// Token-2022 mint carrying a transfer-fee extension — outside the allowlist.
pub fn set_transfer_fee_token_2022_mint(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
) {
    set_token_2022_mint_with_extension(
        svm,
        mint,
        authority,
        decimals,
        EXTENSION_TRANSFER_FEE_CONFIG,
        108,
    );
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

fn set_mint_with_len(
    svm: &mut LiteSVM,
    mint: Pubkey,
    authority: &Pubkey,
    decimals: u8,
    token_program: Pubkey,
    len: usize,
) {
    let mut data = vec![0u8; len];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..36].copy_from_slice(authority.as_ref());
    data[44] = decimals;
    data[45] = 1;
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

/// Overwrite the `supply` field of an initialized SPL mint.
pub fn set_mint_supply(svm: &mut LiteSVM, mint: &Pubkey, supply: u64) {
    let mut account = svm.get_account(mint).unwrap();
    account.data[36..44].copy_from_slice(&supply.to_le_bytes());
    svm.set_account(*mint, account).unwrap();
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

/// Install an initialized token account holding `amount` of `mint`.
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

/// Install an initialized Token-2022 token account holding `amount` of `mint`.
pub fn set_token_2022_account(
    svm: &mut LiteSVM,
    address: Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
) {
    set_token_account_with_program(svm, address, mint, owner, amount, TOKEN_2022_PROGRAM_ID);
}

/// Install + fund the ATA for `(owner, mint)`; returns its address.
pub fn set_ata(svm: &mut LiteSVM, owner: &Pubkey, mint: &Pubkey, amount: u64) -> Pubkey {
    let address = associated_token_address(owner, mint);
    set_token_account(svm, address, mint, owner, amount);
    address
}

/// Install + fund the ATA for `(owner, mint, token_program)`; returns its address.
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

/// Read the `amount` field of an SPL token account.
pub fn token_balance(svm: &LiteSVM, address: &Pubkey) -> u64 {
    let account = svm.get_account(address).unwrap();
    u64::from_le_bytes(account.data[64..72].try_into().unwrap())
}

/// Read the `supply` field of an SPL mint (offset 36, after the 4-byte
/// `mint_authority` COption tag and its 32-byte key).
pub fn mint_supply(svm: &LiteSVM, mint: &Pubkey) -> u64 {
    let account = svm.get_account(mint).unwrap();
    u64::from_le_bytes(account.data[36..44].try_into().unwrap())
}
