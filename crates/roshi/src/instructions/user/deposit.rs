use solana_account_info::AccountInfo;
use solana_program_error::{ProgramError, ProgramResult};

use crate::{
    instructions::{accounts::DepositContext, token, DepositArgs},
    state::vault::Vault,
};
use roshi_interface::{access::MAX_ACCESS_PROOF_LEN, error::RoshiError, math::shares_for_deposit};

/// Implements [`crate::instructions::RoshiInstruction::Deposit`].
///
/// # Accounts
///
/// 0. `[signer]` Depositor.
/// 1. `[writable]` Vault account receiving the deposit accounting update.
/// 2. `[writable]` Depositor source token account for `asset_mint`.
/// 3. `[writable]` Vault custody token account for the selected asset.
/// 4. `[writable]` Depositor share token account.
/// 5. `[writable]` Share mint (`vault.share_mint`).
/// 6. `[]` Classic SPL Token program for share minting.
/// 7. `[]` Deposited asset SPL Token program.
/// 8. `[]` Asset PDA (non-base deposits only).
/// 9. `..` Oracle accounts (non-base deposits only): asset oracle leg, then
///    the vault base oracle leg for routed assets.
///
/// If the vault is private, instruction data must include a Merkle proof for
/// the depositor's wallet against `vault.access_merkle_root`.
///
/// Rejects deposits while paused, gates private vaults by Merkle proof, prices
/// the deposit in base atoms (non-base deposits through the asset oracle —
/// composed with the vault base oracle for routed assets — scaled by mint
/// decimals) with a virtual-share offset against donation inflation, and enforces
/// `min_shares_out` *before* any funds move. Only then does it route base-mint
/// deposits into custody owned by `vault.deposit_sub_account` (non-base
/// deposits into their Asset custody), mint shares to the depositor (vault PDA
/// is the share-mint authority), and increase `total_assets`.
pub fn try_deposit<'info>(
    accounts: &'info [AccountInfo<'info>],
    args: DepositArgs,
) -> ProgramResult {
    if args.access_proof.len() > MAX_ACCESS_PROOF_LEN {
        return Err(RoshiError::InvalidAccessProof.into());
    }

    let context = DepositContext::load(accounts)?;
    let vault = &context.vault;

    if vault.deposits_paused()? {
        return Err(RoshiError::VaultPaused.into());
    }
    if !vault.allows_depositor(context.depositor.key, &args.access_proof) {
        return Err(RoshiError::InvalidAccessProof.into());
    }

    let base_atoms = context.resolve_base_atoms(&args)?;
    let share_supply = token::mint_supply(context.share_mint)?;
    let economic_share_supply = vault.economic_share_supply(share_supply)?;

    let shares = shares_for_deposit(
        base_atoms,
        vault.total_assets,
        economic_share_supply,
        vault.base_decimals,
    )?;
    if shares < args.min_shares_out {
        return Err(RoshiError::SlippageExceeded.into());
    }

    // Pull the deposit into custody (the depositor authorizes the transfer).
    token::transfer(
        context.asset_token_program,
        context.source,
        context.custody,
        context.depositor,
        args.amount,
    )?;

    // Mint shares to the depositor; the vault PDA is the share-mint authority.
    let tag = vault.tag_seed()?;
    let base_mint = vault.base_mint;
    let bump = [vault.bump];
    let signer_seeds: &[&[u8]] = &[Vault::SEED, tag, &base_mint, &bump];
    token::mint_to_signed(
        context.share_token_program,
        context.share_mint,
        context.share_dest,
        context.vault_account,
        shares,
        signer_seeds,
    )?;

    context.store(|vault| {
        vault.total_assets = vault
            .total_assets
            .checked_add(base_atoms)
            .ok_or(ProgramError::from(RoshiError::Overflow))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_access_proof() {
        assert_eq!(
            try_deposit(
                &[],
                DepositArgs {
                    asset_mint: [0; 32],
                    amount: 1,
                    min_shares_out: 1,
                    access_proof: vec![[0; 32]; MAX_ACCESS_PROOF_LEN + 1],
                },
            ),
            Err(ProgramError::from(RoshiError::InvalidAccessProof))
        );
    }
}
