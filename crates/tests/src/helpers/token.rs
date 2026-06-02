use litesvm::LiteSVM;
use solana_pubkey::Pubkey;
use solana_sdk::account::Account;

/// SPL Token program id (classic).
pub const TOKEN_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
/// Associated Token Account program id.
pub const ATA_PROGRAM_ID: Pubkey =
    solana_pubkey::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Derive the associated token account address for `(wallet, mint)`.
pub fn associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), TOKEN_PROGRAM_ID.as_ref(), mint.as_ref()],
        &ATA_PROGRAM_ID,
    )
    .0
}

/// Install an initialized SPL mint with `authority` as the mint authority.
pub fn set_mint(svm: &mut LiteSVM, mint: Pubkey, authority: &Pubkey, decimals: u8) {
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
            owner: TOKEN_PROGRAM_ID,
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
            owner: TOKEN_PROGRAM_ID,
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
