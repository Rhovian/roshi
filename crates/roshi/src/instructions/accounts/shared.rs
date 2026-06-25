use solana_account_info::AccountInfo;
use solana_cpi::{invoke, invoke_signed};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;
use solana_system_interface::instruction::{allocate, assign, create_account, transfer};
use solana_system_interface::program as system_program;
use solana_sysvar::{rent::Rent, Sysvar};

pub(crate) fn next_account<'a, 'info>(
    accounts: &mut impl Iterator<Item = &'a AccountInfo<'info>>,
) -> Result<&'a AccountInfo<'info>, ProgramError> {
    accounts.next().ok_or(ProgramError::NotEnoughAccountKeys)
}

pub(super) fn require_writable_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    require_writable(account)
}

pub(crate) fn require_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

pub(super) fn require_uninitialized_account(account: &AccountInfo) -> ProgramResult {
    // Tolerate a prefunded (lamports-only) PDA: only non-empty data or a
    // non-system owner means the account is actually initialized. Anyone can
    // transfer lamports to a deterministic PDA, so rejecting on lamports alone is
    // a griefing vector; `create_pda_account` absorbs the prefund instead.
    if !account.data_is_empty() || account.owner != &system_program::ID {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    Ok(())
}

/// Create a program-owned PDA at `target`, tolerating a prefunded account.
///
/// A deterministic PDA can be prefunded with lamports by anyone before creation,
/// and `create_account` fails against any lamport-bearing account — so a bare
/// prefund would grief the legitimate creation. An attacker can only ever move
/// lamports to a PDA (allocating data or reassigning the owner needs the PDA's
/// signature, which only this program holds), so the sole reachable prefunded
/// state is "lamports-only, system-owned, empty data". This absorbs it: fund any
/// rent shortfall, then allocate and assign in place under the PDA seeds.
pub(crate) fn create_pda_account<'info>(
    payer: &AccountInfo<'info>,
    target: &AccountInfo<'info>,
    system_program_acc: &AccountInfo<'info>,
    space: usize,
    owner: &Pubkey,
    signer_seeds: &[&[u8]],
) -> ProgramResult {
    let rent_exemption_lamports = Rent::get()?.minimum_balance(space);
    let account_infos = [payer.clone(), target.clone(), system_program_acc.clone()];

    if target.lamports() == 0 {
        let create_account_ix = create_account(
            payer.key,
            target.key,
            rent_exemption_lamports,
            space as u64,
            owner,
        );
        return invoke_signed(&create_account_ix, &account_infos, &[signer_seeds]);
    }

    let shortfall = rent_exemption_lamports.saturating_sub(target.lamports());
    if shortfall > 0 {
        invoke(&transfer(payer.key, target.key, shortfall), &account_infos)?;
    }
    invoke_signed(
        &allocate(target.key, space as u64),
        &account_infos,
        &[signer_seeds],
    )?;
    invoke_signed(&assign(target.key, owner), &account_infos, &[signer_seeds])
}

pub(super) fn require_system_program(account: &AccountInfo) -> ProgramResult {
    if account.key != &system_program::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    Ok(())
}

/// Close `account`: move its lamports to `refund_to`, clear its data, and return
/// ownership to the system program.
pub(crate) fn close_account(account: &AccountInfo, refund_to: &AccountInfo) -> ProgramResult {
    let reclaimed = account.lamports();
    let refund_balance = refund_to.lamports();
    **refund_to.try_borrow_mut_lamports()? = refund_balance
        .checked_add(reclaimed)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    **account.try_borrow_mut_lamports()? = 0;

    account.resize(0)?;
    account.assign(&system_program::ID);

    Ok(())
}
