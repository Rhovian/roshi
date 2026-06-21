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
const TOKEN_ACCOUNT_DELEGATED_AMOUNT: usize = 121;
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
    if account.owner == &TOKEN_2022_PROGRAM_ID {
        verify_token_2022_mint_extensions(&data)?;
    } else if data.len() != MINT_LEN {
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

/// Extended Token-2022 mints pad the base mint layout to the token-account
/// length, store one account-type byte, then TLV entries of
/// `(type: u16 LE, len: u16 LE, value)`.
const MINT_ACCOUNT_TYPE_OFFSET: usize = TOKEN_ACCOUNT_LEN;
const ACCOUNT_TYPE_MINT: u8 = 1;
const EXTENSION_UNINITIALIZED: u16 = 0;
const EXTENSION_METADATA_POINTER: u16 = 18;
const EXTENSION_TOKEN_METADATA: u16 = 19;

/// Allowlist Token-2022 mint extensions: display metadata only.
///
/// `MetadataPointer` and `TokenMetadata` are benign and display-only;
/// everything else — explicitly including transfer fees, transfer hooks,
/// permanent delegates, close authorities, confidential transfers, interest,
/// pausability, and any unknown future type — is rejected (allowlist, not
/// blocklist). Checking at registration time is sound: every dangerous mint
/// extension must be initialized before `InitializeMint`, so a mint that
/// passes here cannot grow one later. The one post-creation growth case is
/// `TokenMetadata` (a realloc), which is why no caller may assume a fixed
/// mint account length.
fn verify_token_2022_mint_extensions(data: &[u8]) -> ProgramResult {
    if data.len() == MINT_LEN {
        return Ok(());
    }

    if data.len() <= MINT_ACCOUNT_TYPE_OFFSET || data[MINT_ACCOUNT_TYPE_OFFSET] != ACCOUNT_TYPE_MINT
    {
        return Err(RoshiError::InvalidMintAccount.into());
    }

    let mut offset = MINT_ACCOUNT_TYPE_OFFSET + 1;
    while offset < data.len() {
        let header_end = offset
            .checked_add(4)
            .ok_or(ProgramError::from(RoshiError::InvalidMintAccount))?;
        if data.len() < header_end {
            return Err(RoshiError::InvalidMintAccount.into());
        }

        let extension_type = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if extension_type == EXTENSION_UNINITIALIZED {
            // End marker; the remainder is allocation padding.
            return Ok(());
        }
        if extension_type != EXTENSION_METADATA_POINTER
            && extension_type != EXTENSION_TOKEN_METADATA
        {
            return Err(RoshiError::InvalidMintAccount.into());
        }

        let length = usize::from(u16::from_le_bytes([data[offset + 2], data[offset + 3]]));
        offset = header_end
            .checked_add(length)
            .ok_or(ProgramError::from(RoshiError::InvalidMintAccount))?;
        if offset > data.len() {
            return Err(RoshiError::InvalidMintAccount.into());
        }
    }

    Ok(())
}

/// Mint of an initialized SPL token account.
pub(crate) fn token_account_mint(account: &AccountInfo) -> Result<Pubkey, ProgramError> {
    let data = account.try_borrow_data()?;
    if data.len() < TOKEN_ACCOUNT_LEN {
        return Err(RoshiError::InvalidTokenAccount.into());
    }
    Pubkey::try_from(&data[TOKEN_ACCOUNT_MINT..TOKEN_ACCOUNT_MINT + 32])
        .map_err(|_| ProgramError::from(RoshiError::InvalidTokenAccount))
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

/// Pre-CPI baseline check for a swap's named input/output endpoint: a
/// subaccount-owned, initialized token account with **no close authority**. Unlike
/// [`verify_custody_account`] it tolerates a **delegate**, because the
/// flash-collateral Multiply grants its flash-repay delegate on the collateral ATA
/// (the swap's output endpoint) before the swap runs.
///
/// Tolerating a *pre-existing* delegate is only safe in tandem with
/// [`verify_swap_endpoint_unchanged`], which the caller MUST run after the route CPI:
/// this function alone would let the route plant or expand a delegate. A close
/// authority can outright rug the account, so it is rejected here and (being part of
/// the snapshot) can never be added by the CPI either.
pub(crate) fn verify_swap_endpoint_custody(
    account: &AccountInfo,
    subaccount: &Pubkey,
) -> ProgramResult {
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

    if has_close_authority(&data) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    Ok(())
}

/// Post-CPI reverify for a swap endpoint: assert the route changed **only the token
/// balance**. `pre` is the endpoint's account data snapshotted immediately before the
/// CPI; every byte outside the amount field (`TOKEN_ACCOUNT_AMOUNT..+8`) must be
/// byte-identical afterward.
///
/// The named endpoints are exempt from the sibling-custody snapshot (their balances
/// legitimately move within the oracle / `max_in` / `min_out` bounds), so this is the
/// only thing standing between the route and a *standing authority* on them: it makes
/// it impossible for the CPI to create, expand, or retarget a delegate, set a close
/// authority, or reassign the account — any of which would outlive the swap's bounded
/// flow. The pre-existing flash-repay delegate is unchanged, so it passes.
pub(crate) fn verify_swap_endpoint_unchanged(pre: &[u8], account: &AccountInfo) -> ProgramResult {
    let post = account.try_borrow_data()?;
    if pre.len() != post.len()
        || pre[..TOKEN_ACCOUNT_AMOUNT] != post[..TOKEN_ACCOUNT_AMOUNT]
        || pre[TOKEN_ACCOUNT_AMOUNT + 8..] != post[TOKEN_ACCOUNT_AMOUNT + 8..]
    {
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

/// Verify a sub-account custody token account carries the intended one-shot
/// flash delegate: owned by `subaccount`, `delegated_amount == expected_amount`
/// (the bound flash borrow), and no close authority. This replaces the generic
/// "no delegate" reverify for a `FlashApprove` action's approved account — the
/// delegate is the intended effect, but it is bounded so a forced `flash_repay`
/// consumes it exactly. The delegate *identity* is not checked: the borrowed `F`
/// is tied to this account (`flash_borrow.destination == approve.source`), so
/// whatever the allowance moves is money the flash deposited here and owes back
/// — the holder of the allowance is irrelevant to soundness.
pub(crate) fn verify_flash_delegate(
    account: &AccountInfo,
    subaccount: &Pubkey,
    expected_amount: u64,
) -> ProgramResult {
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

    // A close authority would survive the bundle as standing control over the
    // account, so it is rejected.
    let close_tag = u32::from_le_bytes(
        data[TOKEN_ACCOUNT_CLOSE_AUTHORITY..TOKEN_ACCOUNT_CLOSE_AUTHORITY + 4]
            .try_into()
            .unwrap(),
    );
    if close_tag != 0 {
        return Err(RoshiError::FlashDelegateMismatch.into());
    }

    let delegated_amount = u64::from_le_bytes(
        data[TOKEN_ACCOUNT_DELEGATED_AMOUNT..TOKEN_ACCOUNT_DELEGATED_AMOUNT + 8]
            .try_into()
            .unwrap(),
    );
    if delegated_amount != expected_amount {
        return Err(RoshiError::FlashDelegateUnbounded.into());
    }

    Ok(())
}

/// Assert a token account carries no delegate and zero delegated amount — the
/// post-bundle backstop for `FlashApprove` (#21). Bound as a sibling after the
/// top-level `flash_repay`, it makes an over-high committed fee fail loudly: SPL
/// only clears a delegate when `delegated_amount` reaches 0, so a residual
/// allowance (Roshi's fee > the lender's) leaves the delegate set and trips this.
pub(crate) fn assert_delegate_cleared(account: &AccountInfo) -> ProgramResult {
    if !is_token_program(account.owner) {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    let data = account.try_borrow_data()?;
    if data.len() < TOKEN_ACCOUNT_LEN || data[TOKEN_ACCOUNT_STATE] != 1 {
        return Err(RoshiError::InvalidTokenAccount.into());
    }

    let delegate_tag = u32::from_le_bytes(
        data[TOKEN_ACCOUNT_DELEGATE..TOKEN_ACCOUNT_DELEGATE + 4]
            .try_into()
            .unwrap(),
    );
    let delegated_amount = u64::from_le_bytes(
        data[TOKEN_ACCOUNT_DELEGATED_AMOUNT..TOKEN_ACCOUNT_DELEGATED_AMOUNT + 8]
            .try_into()
            .unwrap(),
    );
    if delegate_tag != 0 || delegated_amount != 0 {
        return Err(RoshiError::DelegateNotCleared.into());
    }

    Ok(())
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

/// Whether a token account has a close authority set (the COption tag at
/// `TOKEN_ACCOUNT_CLOSE_AUTHORITY` is non-zero). Unlike a delegate, a close
/// authority can drain/close the account, so it is rejected even for swap endpoints.
fn has_close_authority(data: &[u8]) -> bool {
    u32::from_le_bytes(
        data[TOKEN_ACCOUNT_CLOSE_AUTHORITY..TOKEN_ACCOUNT_CLOSE_AUTHORITY + 4]
            .try_into()
            .unwrap(),
    ) != 0
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
