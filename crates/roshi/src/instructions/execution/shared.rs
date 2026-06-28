use solana_account_info::AccountInfo;
use solana_cpi::invoke_signed;
use solana_instruction::{AccountMeta, Instruction};
use solana_instructions_sysvar::{
    get_instruction_relative, load_current_index_checked, load_instruction_at_checked,
};
use solana_program_error::{ProgramError, ProgramResult};
use solana_pubkey::Pubkey;

use crate::{
    instructions::accounts::ValidatedManageAccounts,
    instructions::AccountFlags,
    state::{
        action::{compute_action_hash_from_metas, Action, ActionScope, Op, Ops, ResolvedSibling},
        sub_account::VaultSubAccount,
    },
};
use roshi_interface::error::RoshiError;
use roshi_interface::math::round_with_min1;

/// SPL Token `Approve` instruction discriminator (shared by Token and
/// Token-2022; `Transfer` is 3, `Approve` is 4). A `FlashApprove` action must
/// relay exactly this instruction.
const SPL_APPROVE_TAG: u8 = 4;

pub(crate) struct AuthorizedCpi<'a, 'info> {
    instruction: Instruction,
    // Borrowed from the relay's account list, not cloned: the same `manage_batch`
    // bump-allocator pressure that #24 fixes in the action hash applied here too.
    account_infos: &'a [AccountInfo<'info>],
    vault_key: Pubkey,
    sub_account_key: Pubkey,
    sub_account_index: u8,
    sub_account_bump: u8,
}

impl<'a, 'info> AuthorizedCpi<'a, 'info> {
    pub(crate) fn has_account_meta(&self, key: &Pubkey) -> bool {
        self.instruction
            .accounts
            .iter()
            .any(|meta| &meta.pubkey == key)
    }

    /// Pre-CPI: identify writable custody accounts controlled by the subaccount
    /// and assert that each is clean before the downstream program runs.
    pub(crate) fn scan_subaccount_custody(&self) -> Result<Vec<Pubkey>, ProgramError> {
        // Pre-size to the route's meta count: this runs once per relayed CPI, so
        // in a `manage_batch` a growing `Vec` would leak its discarded buffers
        // into the never-freed per-instruction heap on every leg (see #24).
        let mut keys = Vec::with_capacity(self.instruction.accounts.len());
        for (meta, info) in self.instruction.accounts.iter().zip(self.account_infos) {
            if meta.is_writable
                && crate::instructions::token::is_clean_custody(info, &self.sub_account_key)?
            {
                keys.push(*info.key);
            }
        }

        Ok(keys)
    }

    /// Post-CPI: re-check the pre-identified custody accounts by key.
    pub(crate) fn reverify_subaccount_custody(&self, keys: &[Pubkey]) -> ProgramResult {
        self.reverify_subaccount_custody_except(keys, None)
    }

    /// Post-CPI reverify that skips `exempt` (a `FlashApprove` action's approved
    /// account, whose delegate is intentional and is checked separately by
    /// [`Self::verify_flash_delegate`]). Every other account is still required
    /// to remain clean.
    fn reverify_subaccount_custody_except(
        &self,
        keys: &[Pubkey],
        exempt: Option<&Pubkey>,
    ) -> ProgramResult {
        for key in keys {
            if exempt == Some(key) {
                continue;
            }
            let info = self
                .account_infos
                .iter()
                .find(|info| info.key == key)
                .ok_or(ProgramError::from(RoshiError::InvalidTokenAccount))?;
            if !crate::instructions::token::is_clean_custody(info, &self.sub_account_key)? {
                return Err(RoshiError::InvalidTokenAccount.into());
            }
        }

        Ok(())
    }

    /// Pre-CPI: snapshot every writable sub-account custody in the CPI account
    /// section as `(key, balance)`, skipping `exempt` (the named swap endpoints,
    /// whose flow the oracle value bound governs). Deduped — aggregator routes
    /// list the same account more than once.
    pub(crate) fn snapshot_writable_custody(
        &self,
        exempt: &[Pubkey],
    ) -> Result<Vec<(Pubkey, u64)>, ProgramError> {
        // Pre-size to the route's meta count (the upper bound on custody here).
        // The bump allocator never frees within an instruction, so a growing
        // `Vec` would leak every discarded buffer — one sized allocation instead.
        let mut snapshot: Vec<(Pubkey, u64)> = Vec::with_capacity(self.instruction.accounts.len());
        for (meta, info) in self.instruction.accounts.iter().zip(self.account_infos) {
            if !meta.is_writable
                || exempt.contains(info.key)
                || snapshot.iter().any(|(key, _)| key == info.key)
            {
                continue;
            }
            if crate::instructions::token::is_clean_custody(info, &self.sub_account_key)? {
                let amount = crate::instructions::token::token_amount(info)?;
                snapshot.push((*info.key, amount));
            }
        }

        Ok(snapshot)
    }

    /// Post-CPI: every snapshotted custody must still be clean and hold the same
    /// balance — an authorized route may only move value through the named
    /// endpoints, never a sibling custody it was also handed. A lingering
    /// delegate (balance flat but a later drain enabled) is caught by the clean
    /// check, a debit by the balance check.
    pub(crate) fn verify_custody_unchanged(&self, snapshot: &[(Pubkey, u64)]) -> ProgramResult {
        for (key, amount) in snapshot {
            let info = self
                .account_infos
                .iter()
                .find(|info| info.key == key)
                .ok_or(ProgramError::from(RoshiError::InvalidTokenAccount))?;
            if !crate::instructions::token::is_clean_custody(info, &self.sub_account_key)? {
                return Err(RoshiError::InvalidTokenAccount.into());
            }
            if crate::instructions::token::token_amount(info)? != *amount {
                return Err(RoshiError::SwapCustodyMoved.into());
            }
        }

        Ok(())
    }

    /// Post-validation: every *writable* account in the CPI must be pinned by the
    /// action hash (folded via an [`Op::IngestAccount`]).
    ///
    /// `AtomicRedeem` is the one relay scope an untrusted public caller drives, so
    /// its route must leave the caller no account freedom. Only ingested accounts
    /// are committed to the action hash; an un-ingested writable meta is therefore
    /// free for the caller to substitute, which lets them redirect the unwind to
    /// drain the source into an account of their choosing (the base-custody
    /// measurement would read `0` and debit no NAV). Requiring *all* writable
    /// metas to be ingested pins the whole moving set — source, destination, and
    /// any other writable — so the public caller can only execute the exact
    /// authorized route. The per-user payout recipient is settled by Roshi
    /// outside this CPI, so it is never a CPI meta and the action stays reusable
    /// across users.
    pub(crate) fn require_writable_metas_bound(&self, ops: &Ops) -> ProgramResult {
        for (index, meta) in self.instruction.accounts.iter().enumerate() {
            if meta.is_writable && !ingests_account(ops, index)? {
                return Err(RoshiError::UnboundAtomicRedeemAccount.into());
            }
        }

        Ok(())
    }

    /// Assert the relayed CPI is an SPL `approve` (program is an SPL token
    /// program, leading data byte is the approve discriminator). A
    /// `FlashApprove` action may only ever grant a delegate via `approve`.
    fn verify_is_approve(&self) -> ProgramResult {
        if !crate::instructions::token::is_token_program(&self.instruction.program_id) {
            return Err(RoshiError::InvalidTokenAccount.into());
        }
        if self.instruction.data.first() != Some(&SPL_APPROVE_TAG) {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(())
    }

    /// The approve's source token account = its first CPI account.
    fn approve_source(&self) -> Result<&AccountInfo<'info>, ProgramError> {
        self.account_infos
            .first()
            .ok_or(ProgramError::NotEnoughAccountKeys)
    }

    /// Post-CPI: the approved source carries the bounded one-shot delegate —
    /// `delegated_amount == expected_amount`, no close authority.
    fn verify_flash_delegate(&self, expected_amount: u64) -> ProgramResult {
        let source = self.approve_source()?;
        crate::instructions::token::verify_flash_delegate(
            source,
            &self.sub_account_key,
            expected_amount,
        )
    }
}

/// Whether `ops` commits the CPI account at `index` via [`Op::IngestAccount`],
/// folding its pubkey and meta flags into the action hash.
fn ingests_account(ops: &Ops, index: usize) -> Result<bool, ProgramError> {
    for op in ops.iter().map_err(|_| RoshiError::InvalidOp)? {
        if let Op::IngestAccount { index: ingested } = op.map_err(|_| RoshiError::InvalidOp)? {
            if usize::from(ingested) == index {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Validates and prepares one pre-authorized downstream CPI.
///
/// # Accounts
///
/// `cpi_accounts` is the remaining account section after the Roshi instruction
/// prefix has been consumed. `accounts_start` and `accounts_len` select the
/// downstream CPI account metas relative to that section. The target program
/// account must be supplied immediately after the selected CPI account metas;
/// it must be executable and is passed through to `invoke_signed` as an
/// account info but is not included as an instruction meta.
///
/// # Implementation
///
/// Rebuilds the intended CPI metas from selected CPI accounts plus explicit
/// flags, then recomputes the action hash from the effective CPI program id,
/// stored `Ops`, rebuilt metas, and instruction data. The CPI program id is
/// derived from the program account supplied immediately after the selected
/// metas (no longer re-sent in args); the action hash commits it, so
/// substituting a different program account fails the hash check. The selected
/// subaccount is promoted to signer when present in the CPI metas.
pub(crate) fn validate_authorized_cpi<'a, 'info>(
    cpi_accounts: &'a [AccountInfo<'info>],
    validated_accounts: &ValidatedManageAccounts,
    accounts_start: u8,
    accounts_len: u8,
    account_flags: Vec<AccountFlags>,
    ix_data: Vec<u8>,
) -> Result<AuthorizedCpi<'a, 'info>, ProgramError> {
    let accounts_start = usize::from(accounts_start);
    let accounts_len = usize::from(accounts_len);
    let accounts_end = accounts_start
        .checked_add(accounts_len)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let cpi_meta_accounts = cpi_accounts
        .get(accounts_start..accounts_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    // The CPI program is the account supplied immediately after the metas. Its
    // key is folded into the action hash below, so a mismatched program account
    // is rejected by the hash check rather than a direct comparison to args.
    let cpi_program_acc = cpi_accounts
        .get(accounts_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;
    let cpi_program_id = *cpi_program_acc.key;
    if !cpi_program_acc.executable {
        return Err(ProgramError::InvalidAccountData);
    }
    if account_flags.len() != accounts_len {
        return Err(ProgramError::InvalidInstructionData);
    }

    let cpi_account_metas = cpi_meta_accounts
        .iter()
        .zip(account_flags)
        .map(|(acc, flags)| {
            if flags.is_writable && !acc.is_writable {
                return Err(ProgramError::InvalidAccountData);
            }

            let is_sub_account = acc.key == &validated_accounts.sub_account_key;
            if flags.is_signer && !acc.is_signer && !is_sub_account {
                return Err(ProgramError::MissingRequiredSignature);
            }

            let is_signer = flags.is_signer || is_sub_account;
            if flags.is_writable {
                Ok(AccountMeta::new(*acc.key, is_signer))
            } else {
                Ok(AccountMeta::new_readonly(*acc.key, is_signer))
            }
        })
        .collect::<Result<Vec<_>, ProgramError>>()?;

    let loaded_siblings = load_required_siblings(&validated_accounts.action.ops, cpi_accounts)?;
    let resolved_siblings = loaded_siblings
        .iter()
        .map(LoadedSibling::as_resolved)
        .collect::<Vec<_>>();

    let action_hash = compute_action_hash_from_metas(
        &cpi_program_id,
        &validated_accounts.action.ops,
        &cpi_account_metas,
        &ix_data,
        &resolved_siblings,
    )?;
    if validated_accounts.action.action_hash != action_hash {
        return Err(RoshiError::UnauthorizedAction.into());
    }

    let account_infos_end = accounts_end
        .checked_add(1)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let cpi_account_infos = cpi_accounts
        .get(accounts_start..account_infos_end)
        .ok_or(ProgramError::NotEnoughAccountKeys)?;

    Ok(AuthorizedCpi {
        instruction: Instruction {
            program_id: cpi_program_id,
            accounts: cpi_account_metas,
            data: ix_data,
        },
        account_infos: cpi_account_infos,
        vault_key: validated_accounts.vault_key,
        sub_account_key: validated_accounts.sub_account_key,
        sub_account_index: validated_accounts.sub_account_index,
        sub_account_bump: validated_accounts.sub_account_bump,
    })
}

/// A top-level sibling instruction read from the instructions sysvar, owning
/// the fields the action hash folds. Kept alive while [`ResolvedSibling`]
/// borrows from it for the hash computation.
struct LoadedSibling {
    relative_index: i8,
    program_id: Pubkey,
    data: Vec<u8>,
    account_keys: Vec<Pubkey>,
}

impl LoadedSibling {
    fn as_resolved(&self) -> ResolvedSibling<'_> {
        ResolvedSibling {
            relative_index: self.relative_index,
            program_id: self.program_id,
            data: &self.data,
            accounts: &self.account_keys,
        }
    }
}

/// Locate the instructions sysvar among the relay's accounts and confirm Roshi
/// is the executing top-level instruction. Sibling ops address by index relative
/// to the executing top-level instruction (`get_instruction_relative`); if
/// `manage`/`manage_batch` is itself reached via CPI from a wrapper program, that
/// index would be relative to the wrapper, not Roshi — so sibling-bound actions
/// require top-level execution.
fn instructions_sysvar<'a, 'info>(
    cpi_accounts: &'a [AccountInfo<'info>],
) -> Result<&'a AccountInfo<'info>, ProgramError> {
    let sysvar = cpi_accounts
        .iter()
        .find(|account| account.key == &solana_sdk_ids::sysvar::instructions::ID)
        .ok_or(RoshiError::MissingInstructionsSysvar)?;

    let current = load_current_index_checked(sysvar)?;
    let executing = load_instruction_at_checked(usize::from(current), sysvar)?;
    if executing.program_id != crate::ID {
        return Err(RoshiError::SiblingsRequireTopLevel.into());
    }

    Ok(sysvar)
}

/// Reads every top-level sibling instruction referenced by the action's sibling
/// ops. The instructions sysvar is located by id among the relay's accounts;
/// it is only required when a sibling op is present, so non-sibling relays are
/// unaffected. Each distinct `relative_index` is resolved against the executing
/// top-level instruction (the `manage`/`manage_batch` call).
fn load_required_siblings(
    ops: &Ops,
    cpi_accounts: &[AccountInfo],
) -> Result<Vec<LoadedSibling>, ProgramError> {
    let mut relative_indices: Vec<i8> = Vec::new();
    for op in ops.iter().map_err(|_| RoshiError::InvalidOp)? {
        let relative_index = match op.map_err(|_| RoshiError::InvalidOp)? {
            Op::IngestSiblingInstruction { relative_index, .. }
            | Op::IngestSiblingAccount { relative_index, .. } => relative_index,
            _ => continue,
        };
        if !relative_indices.contains(&relative_index) {
            relative_indices.push(relative_index);
        }
    }

    if relative_indices.is_empty() {
        return Ok(Vec::new());
    }

    let sysvar = instructions_sysvar(cpi_accounts)?;

    let mut loaded = Vec::with_capacity(relative_indices.len());
    for relative_index in relative_indices {
        let instruction = get_instruction_relative(i64::from(relative_index), sysvar)
            .map_err(|_| RoshiError::RequiredSiblingMissing)?;
        let account_keys = instruction
            .accounts
            .iter()
            .map(|meta| meta.pubkey)
            .collect();
        loaded.push(LoadedSibling {
            relative_index,
            program_id: instruction.program_id,
            data: instruction.data,
            account_keys,
        });
    }

    Ok(loaded)
}

/// Invokes a CPI after all Roshi and CPI-specific authorization checks have
/// already been performed.
pub(crate) fn invoke_authorized_cpi(authorized_cpi: &AuthorizedCpi<'_, '_>) -> ProgramResult {
    let sub_account_index_seed = [authorized_cpi.sub_account_index];
    let sub_account_bump_seed = [authorized_cpi.sub_account_bump];
    let signer_seeds = &[
        VaultSubAccount::SEED,
        authorized_cpi.vault_key.as_ref(),
        &sub_account_index_seed,
        &sub_account_bump_seed,
    ];

    invoke_signed(
        &authorized_cpi.instruction,
        authorized_cpi.account_infos,
        &[signer_seeds],
    )
}

/// Run the post-validation custody settlement for one authorized CPI, dispatched
/// by the action's scope. `Manager` uses the standard pre/post custody
/// scan/reverify. `FlashApprove` relays an SPL `approve`, exempts the approved
/// account from the standard reverify, and instead binds its one-shot delegate
/// to the bound flash-borrow amount so a forced `flash_repay` consumes it
/// exactly. Reachable scopes are gated to the strategist by
/// `verify_action_executor`; `Swap`/`AtomicRedeem` never relay here.
pub(crate) fn settle_authorized_cpi(
    authorized_cpi: &AuthorizedCpi<'_, '_>,
    action: &Action,
    cpi_accounts: &[AccountInfo],
) -> ProgramResult {
    match action.scope {
        ActionScope::Manager => {
            let custody = authorized_cpi.scan_subaccount_custody()?;
            invoke_authorized_cpi(authorized_cpi)?;
            authorized_cpi.reverify_subaccount_custody(&custody)
        }
        ActionScope::FlashApprove => {
            authorized_cpi.verify_is_approve()?;
            let source_key = *authorized_cpi.approve_source()?.key;
            // Tie the flash-borrow's destination and the cleared-check's target
            // to the delegated account, and read the borrowed `F`.
            let flash_amount = bind_flash_siblings(&action.ops, cpi_accounts, &source_key)?;
            // Bind the allowance to `F + round_half_up(F * fee_num / fee_den)`
            // floored at one atom — the committed opaque rate reproduces the
            // lender's proportional flash fee bit-for-bit (klend rounds, not
            // ceils; see #25), so the forced `flash_repay` clears the delegate.
            let fee = round_with_min1(flash_amount, action.fee_num, action.fee_den)?;
            let expected = flash_amount.checked_add(fee).ok_or(RoshiError::Overflow)?;
            let custody = authorized_cpi.scan_subaccount_custody()?;
            invoke_authorized_cpi(authorized_cpi)?;
            authorized_cpi.reverify_subaccount_custody_except(&custody, Some(&source_key))?;
            authorized_cpi.verify_flash_delegate(expected)
        }
        ActionScope::Swap | ActionScope::AtomicRedeem => Err(RoshiError::UnauthorizedAction.into()),
    }
}

/// Resolve the two siblings a `FlashApprove` action commits, tie each to the
/// delegated `source`, and return the flash-borrowed `F`.
///
/// The action commits, via the #18 ops, exactly two siblings — each an
/// `IngestSiblingInstruction` (program + selector, with `F` right after the
/// flash-borrow's selector) paired with an `IngestSiblingAccount` (one of its
/// accounts) at the same relative index, disambiguated by resolved program:
///
/// - the **flash-borrow** (`program != crate::ID`): `F` lands in the delegated
///   account, so its tied account must equal `source`; `F` is read after the
///   committed selector;
/// - the **`assert_delegate_cleared` backstop** (`program == crate::ID`): a
///   committed, un-omittable post-`flash_repay` check that the same `source`
///   ends with no delegate, so an over-high committed fee can't leave a residual.
///
/// Program + selector + the tied account are pinned by the action hash; this
/// adds the runtime tie of each sibling's account to `source`.
fn bind_flash_siblings(
    ops: &Ops,
    cpi_accounts: &[AccountInfo],
    source: &Pubkey,
) -> Result<u64, ProgramError> {
    // (relative_index, amount_offset) and (relative_index, account_index).
    let mut instruction_ops: Vec<(i8, usize)> = Vec::new();
    let mut account_ops: Vec<(i8, usize)> = Vec::new();
    for op in ops.iter().map_err(|_| RoshiError::InvalidOp)? {
        match op.map_err(|_| RoshiError::InvalidOp)? {
            Op::IngestSiblingInstruction {
                relative_index,
                offset,
                len,
            } => instruction_ops.push((relative_index, usize::from(offset) + usize::from(len))),
            Op::IngestSiblingAccount {
                relative_index,
                index,
            } => account_ops.push((relative_index, usize::from(index))),
            _ => {}
        }
    }
    // Exactly two committed siblings, each instruction op paired with an account op.
    if instruction_ops.len() != 2 || account_ops.len() != 2 {
        return Err(RoshiError::InvalidOp.into());
    }

    let sysvar = instructions_sysvar(cpi_accounts)?;

    let mut flash_amount: Option<u64> = None;
    let mut cleared_check_seen = false;
    for (relative_index, amount_offset) in instruction_ops {
        let (_, account_index) = account_ops
            .iter()
            .copied()
            .find(|(rel, _)| *rel == relative_index)
            .ok_or(RoshiError::InvalidOp)?;
        let instruction = get_instruction_relative(i64::from(relative_index), sysvar)
            .map_err(|_| RoshiError::RequiredSiblingMissing)?;

        // Both siblings tie an account to the delegated source: the flash-borrow
        // its destination, the cleared-check its checked account.
        let tied = instruction
            .accounts
            .get(account_index)
            .ok_or(ProgramError::from(RoshiError::AccountIndexOutOfBounds))?
            .pubkey;
        if &tied != source {
            return Err(RoshiError::FlashDestinationMismatch.into());
        }

        if instruction.program_id == crate::ID {
            // The assert_delegate_cleared backstop — required present, no amount.
            if cleared_check_seen {
                return Err(RoshiError::InvalidOp.into());
            }
            cleared_check_seen = true;
        } else {
            // The flash-borrow — read `F` immediately after the committed selector.
            if flash_amount.is_some() {
                return Err(RoshiError::InvalidOp.into());
            }
            let end = amount_offset
                .checked_add(8)
                .ok_or(ProgramError::InvalidInstructionData)?;
            let bytes = instruction
                .data
                .get(amount_offset..end)
                .ok_or(ProgramError::from(RoshiError::InstructionSliceOutOfBounds))?;
            flash_amount = Some(u64::from_le_bytes(bytes.try_into().unwrap()));
        }
    }

    match (flash_amount, cleared_check_seen) {
        (Some(amount), true) => Ok(amount),
        _ => Err(RoshiError::RequiredSiblingMissing.into()),
    }
}
