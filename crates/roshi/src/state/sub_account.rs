use solana_pubkey::Pubkey;

/// PDA signer namespace for vault custody and strategy execution.
///
/// Subaccounts are intentionally not Roshi-owned data accounts. They are PDA
/// authorities that can own token accounts and sign authorized CPIs.
pub struct VaultSubAccount;

impl VaultSubAccount {
    pub const SEED: &'static [u8] = b"sub_account";

    pub fn find_address(vault: &Pubkey, index: u8) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED, vault.as_ref(), &[index]], &crate::ID)
    }
}
