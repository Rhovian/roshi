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
const MINT_DECIMALS: usize = 44;
const MINT_IS_INITIALIZED: usize = 45;
const MINT_LEN: usize = 82;

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
