//! Shared Roshi interface types.

pub mod access;
pub mod action;
pub mod error;
pub mod instructions;
pub mod math;
pub mod oracle;

solana_pubkey::declare_id!("RoshianbALLAs1RzbvHSHpLRaA8ayaKERQCbfmLb9UP");

pub const SHARE_MINT_SEED: &[u8] = b"share_mint";

pub fn find_share_mint_address(vault: &solana_pubkey::Pubkey) -> (solana_pubkey::Pubkey, u8) {
    solana_pubkey::Pubkey::find_program_address(&[SHARE_MINT_SEED, vault.as_ref()], &ID)
}
