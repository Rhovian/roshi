use solana_account_info::AccountInfo;
use solana_cpi::{invoke, invoke_signed};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::{pubkey, Pubkey};

use roshi_interface::error::RoshiError;

/// SPL Token program id.
pub(crate) const TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
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
const TOKEN_ACCOUNT_STATE: usize = 108;
const TOKEN_ACCOUNT_LEN: usize = 165;

/// Derive the associated token account address for `wallet` and `mint` under
/// the classic SPL Token program.
pub(crate) fn associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), TOKEN_PROGRAM_ID.as_ref(), mint.as_ref()],
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
    if account.key != expected_key || account.owner != &TOKEN_PROGRAM_ID {
        return Err(RoshiError::InvalidMintAccount.into());
    }

    let data = account.try_borrow_data()?;
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

/// Verify `account` is the classic SPL Token program.
pub(crate) fn verify_token_program(account: &AccountInfo) -> ProgramResult {
    if account.key != &TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    Ok(())
}

/// Read the supply of an initialized SPL mint.
pub(crate) fn mint_supply(account: &AccountInfo) -> Result<u64, ProgramError> {
    if account.owner != &TOKEN_PROGRAM_ID {
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
    if account.owner != &TOKEN_PROGRAM_ID {
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

/// Read the amount field of an initialized SPL token account.
pub(crate) fn token_amount(account: &AccountInfo) -> Result<u64, ProgramError> {
    if account.owner != &TOKEN_PROGRAM_ID {
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
    let instruction = spl_token_interface::instruction::transfer(
        token_program.key,
        source.key,
        destination.key,
        authority.key,
        &[],
        amount,
    )?;

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
    let instruction = spl_token_interface::instruction::transfer(
        token_program.key,
        source.key,
        destination.key,
        authority.key,
        &[],
        amount,
    )?;

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
    let instruction = spl_token_interface::instruction::burn(
        token_program.key,
        account.key,
        mint.key,
        authority.key,
        &[],
        amount,
    )?;

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
    let instruction = spl_token_interface::instruction::mint_to(
        token_program.key,
        mint.key,
        destination.key,
        mint_authority.key,
        &[],
        amount,
    )?;

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
