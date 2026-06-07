use solana_account_info::AccountInfo;
use solana_cpi::{invoke, invoke_signed};
use solana_instruction::{AccountMeta, Instruction};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::{pubkey, Pubkey};

use roshi_interface::error::RoshiError;

/// SPL Token program id.
pub(crate) const TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
/// Token-2022 program id.
pub(crate) const TOKEN_2022_PROGRAM_ID: Pubkey =
    pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
/// Associated Token Account program id.
pub(crate) const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey =
    pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Byte offsets into an SPL `Mint` (`mint_authority` COption, `decimals`,
/// `is_initialized`).
const MINT_AUTHORITY_TAG: usize = 0;
const MINT_AUTHORITY_KEY: usize = 4;
const MINT_SUPPLY: usize = 36;
const MINT_DECIMALS: usize = 44;
const MINT_IS_INITIALIZED: usize = 45;
pub(crate) const MINT_LEN: usize = 82;
const TOKEN_ACCOUNT_MINT: usize = 0;
const TOKEN_ACCOUNT_OWNER: usize = 32;
const TOKEN_ACCOUNT_AMOUNT: usize = 64;
const TOKEN_ACCOUNT_DELEGATE: usize = 72;
const TOKEN_ACCOUNT_STATE: usize = 108;
const TOKEN_ACCOUNT_CLOSE_AUTHORITY: usize = 129;
const TOKEN_ACCOUNT_LEN: usize = 165;
const TRANSFER_INSTRUCTION: u8 = 3;
const MINT_TO_INSTRUCTION: u8 = 7;
const BURN_INSTRUCTION: u8 = 8;

pub(crate) fn is_token_program(key: &Pubkey) -> bool {
    key == &TOKEN_PROGRAM_ID || key == &TOKEN_2022_PROGRAM_ID
}

/// Derive the associated token account address for `wallet` and `mint`.
pub(crate) fn associated_token_address(
    wallet: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

/// Verify `account` is an initialized SPL mint at `expected_key` with the given
/// `decimals`, and (when `expected_authority` is set) that mint authority.
pub(crate) fn verify_mint(
    account: &AccountInfo,
    expected_key: &Pubkey,
    decimals: u8,
    expected_authority: Option<&Pubkey>,
) -> ProgramResult {
    if account.key != expected_key || !is_token_program(account.owner) {
        return Err(RoshiError::InvalidMintAccount.into());
    }

    let data = account.try_borrow_data()?;
    if account.owner == &TOKEN_2022_PROGRAM_ID && data.len() != MINT_LEN {
        return Err(RoshiError::InvalidMintAccount.into());
    }
    if data.len() < MINT_LEN || data[MINT_IS_INITIALIZED] != 1 || data[MINT_DECIMALS] != decimals {
        return Err(RoshiError::InvalidMintAccount.into());
    }

    if let Some(authority) = expected_authority {
        let has_authority = u32::from_le_bytes(
            data[MINT_AUTHORITY_TAG..MINT_AUTHORITY_TAG + 4]
                .try_into()
                .unwrap(),
        ) == 1;
        let stored = Pubkey::try_from(&data[MINT_AUTHORITY_KEY..MINT_AUTHORITY_KEY + 32])
            .map_err(|_| ProgramError::from(RoshiError::InvalidMintAccount))?;
        if !has_authority || &stored != authority {
            return Err(RoshiError::InvalidMintAccount.into());
        }
    }

    Ok(())
}

/// Verify `account` is an accepted SPL Token program.
pub(crate) fn verify_token_program(account: &AccountInfo) -> ProgramResult {
    if !is_token_program(account.key) {
        return Err(ProgramError::IncorrectProgramId);
    }

    Ok(())
}

/// Verify `token_program` is the program that owns `token_account`.
pub(crate) fn verify_token_program_for(
    token_program: &AccountInfo,
    token_account: &AccountInfo,
) -> ProgramResult {
    verify_token_program(token_program)?;
    if token_account.owner != token_program.key {
        return Err(ProgramError::IncorrectProgramId);
    }

    Ok(())
}

/// Read the supply of an initialized SPL mint.
pub(crate) fn mint_supply(account: &AccountInfo) -> Result<u64, ProgramError> {
    if !is_token_program(account.owner) {
        return Err(RoshiError::InvalidMintAccount.into());
    }

    let data = account.try_borrow_data()?;
    if data.len() < MINT_LEN || data[MINT_IS_INITIALIZED] != 1 {
        return Err(RoshiError::InvalidMintAccount.into());
    }

    Ok(u64::from_le_bytes(
        data[MINT_SUPPLY..MINT_SUPPLY + 8]
            .try_into()
            .map_err(|_| ProgramError::from(RoshiError::InvalidMintAccount))?,
    ))
}

/// CPI an SPL `initialize_mint2` for a freshly created mint account.
pub(crate) fn initialize_mint<'info>(
    token_program: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    mint_authority: &Pubkey,
    decimals: u8,
) -> ProgramResult {
    if token_program.key != &TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let instruction = spl_token_interface::instruction::initialize_mint2(
        token_program.key,
        mint.key,
        mint_authority,
        None,
        decimals,
    )?;

    invoke(&instruction, &[mint.clone(), token_program.clone()])
}

/// Verify `account` is an initialized SPL token account for `expected_mint`.
pub(crate) fn verify_token_account_mint(
    account: &AccountInfo,
    expected_mint: &Pubkey,
) -> ProgramResult {
    if !is_token_program(account.owner) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    let data = account.try_borrow_data()?;
    if data.len() < TOKEN_ACCOUNT_LEN || data[TOKEN_ACCOUNT_STATE] != 1 {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    let mint = Pubkey::try_from(&data[TOKEN_ACCOUNT_MINT..TOKEN_ACCOUNT_MINT + 32])
        .map_err(|_| ProgramError::from(RoshiError::InvalidTokenAccount))?;
    if &mint != expected_mint {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    Ok(())
}

/// Verify `account` is an initialized SPL token account for `expected_mint`
/// owned by `expected_owner`.
pub(crate) fn verify_token_account_mint_and_owner(
    account: &AccountInfo,
    expected_mint: &Pubkey,
    expected_owner: &Pubkey,
) -> ProgramResult {
    verify_token_account_mint(account, expected_mint)?;

    let data = account.try_borrow_data()?;
    let owner = Pubkey::try_from(&data[TOKEN_ACCOUNT_OWNER..TOKEN_ACCOUNT_OWNER + 32])
        .map_err(|_| ProgramError::from(RoshiError::InvalidTokenAccount))?;
    if &owner != expected_owner {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    Ok(())
}

/// Verify `account` is an initialized SPL token account fully controlled by
/// `subaccount`, independent of mint.
pub(crate) fn verify_custody_account(account: &AccountInfo, subaccount: &Pubkey) -> ProgramResult {
    if !is_token_program(account.owner) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    let data = account.try_borrow_data()?;
    if data.len() < TOKEN_ACCOUNT_LEN || data[TOKEN_ACCOUNT_STATE] != 1 {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    let owner = Pubkey::try_from(&data[TOKEN_ACCOUNT_OWNER..TOKEN_ACCOUNT_OWNER + 32])
        .map_err(|_| ProgramError::from(RoshiError::InvalidTokenAccount))?;
    if &owner != subaccount {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    if has_transfer_authority(&data) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    Ok(())
}

/// Classify `account` as the subaccount's custody for the control scan.
///
/// Returns `Ok(true)` for a clean subaccount-owned token account, `Ok(false)`
/// for accounts outside that custody set, and errors when a subaccount-owned
/// token account has delegated transfer or close authority.
pub(crate) fn is_clean_custody(
    account: &AccountInfo,
    subaccount: &Pubkey,
) -> Result<bool, ProgramError> {
    if !is_token_program(account.owner) {
        return Ok(false);
    }

    let data = account.try_borrow_data()?;
    if data.len() < TOKEN_ACCOUNT_LEN || data[TOKEN_ACCOUNT_STATE] != 1 {
        return Ok(false);
    }

    let owner = Pubkey::try_from(&data[TOKEN_ACCOUNT_OWNER..TOKEN_ACCOUNT_OWNER + 32])
        .map_err(|_| ProgramError::from(RoshiError::InvalidTokenAccount))?;
    if &owner != subaccount {
        return Ok(false);
    }

    if has_transfer_authority(&data) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    Ok(true)
}

fn has_transfer_authority(data: &[u8]) -> bool {
    let delegate_tag = u32::from_le_bytes(
        data[TOKEN_ACCOUNT_DELEGATE..TOKEN_ACCOUNT_DELEGATE + 4]
            .try_into()
            .unwrap(),
    );
    let close_tag = u32::from_le_bytes(
        data[TOKEN_ACCOUNT_CLOSE_AUTHORITY..TOKEN_ACCOUNT_CLOSE_AUTHORITY + 4]
            .try_into()
            .unwrap(),
    );

    delegate_tag != 0 || close_tag != 0
}

/// Read the amount field of an initialized SPL token account.
pub(crate) fn token_amount(account: &AccountInfo) -> Result<u64, ProgramError> {
    if !is_token_program(account.owner) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    let data = account.try_borrow_data()?;
    if data.len() < TOKEN_ACCOUNT_LEN || data[TOKEN_ACCOUNT_STATE] != 1 {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    Ok(u64::from_le_bytes(
        data[TOKEN_ACCOUNT_AMOUNT..TOKEN_ACCOUNT_AMOUNT + 8]
            .try_into()
            .map_err(|_| ProgramError::from(RoshiError::InvalidTokenAccount))?,
    ))
}

/// CPI an SPL token transfer authorized by `authority` (a transaction signer).
pub(crate) fn transfer<'info>(
    token_program: &AccountInfo<'info>,
    source: &AccountInfo<'info>,
    destination: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
) -> ProgramResult {
    let instruction = token_amount_instruction(
        token_program.key,
        TRANSFER_INSTRUCTION,
        source.key,
        destination.key,
        authority.key,
        amount,
    );

    invoke(
        &instruction,
        &[
            source.clone(),
            destination.clone(),
            authority.clone(),
            token_program.clone(),
        ],
    )
}

/// CPI an SPL token transfer authorized by a PDA via `signer_seeds`.
pub(crate) fn transfer_signed<'info>(
    token_program: &AccountInfo<'info>,
    source: &AccountInfo<'info>,
    destination: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[u8]],
) -> ProgramResult {
    let instruction = token_amount_instruction(
        token_program.key,
        TRANSFER_INSTRUCTION,
        source.key,
        destination.key,
        authority.key,
        amount,
    );

    invoke_signed(
        &instruction,
        &[
            source.clone(),
            destination.clone(),
            authority.clone(),
            token_program.clone(),
        ],
        &[signer_seeds],
    )
}

/// CPI an SPL token burn authorized by `authority` (a transaction signer that
/// owns `account`).
pub(crate) fn burn<'info>(
    token_program: &AccountInfo<'info>,
    account: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
) -> ProgramResult {
    let instruction = token_amount_instruction(
        token_program.key,
        BURN_INSTRUCTION,
        account.key,
        mint.key,
        authority.key,
        amount,
    );

    invoke(
        &instruction,
        &[
            account.clone(),
            mint.clone(),
            authority.clone(),
            token_program.clone(),
        ],
    )
}

/// CPI an SPL `mint_to` signed by a PDA `mint_authority` via `signer_seeds`.
pub(crate) fn mint_to_signed<'info>(
    token_program: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    destination: &AccountInfo<'info>,
    mint_authority: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[u8]],
) -> ProgramResult {
    let instruction = token_amount_instruction(
        token_program.key,
        MINT_TO_INSTRUCTION,
        mint.key,
        destination.key,
        mint_authority.key,
        amount,
    );

    invoke_signed(
        &instruction,
        &[
            mint.clone(),
            destination.clone(),
            mint_authority.clone(),
            token_program.clone(),
        ],
        &[signer_seeds],
    )
}

fn token_amount_instruction(
    token_program: &Pubkey,
    instruction_tag: u8,
    first: &Pubkey,
    second: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Instruction {
    let mut data = Vec::with_capacity(9);
    data.push(instruction_tag);
    data.extend_from_slice(&amount.to_le_bytes());

    Instruction {
        program_id: *token_program,
        accounts: vec![
            AccountMeta::new(*first, false),
            AccountMeta::new(*second, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}
