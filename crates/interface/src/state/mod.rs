//! Canonical Roshi account layouts and their account-free logic.
//!
//! Currently the `Vault` — the single source of truth for its data, validation,
//! and PDA derivation, shared by the on-chain program and off-chain readers.

pub mod flags;
pub mod vault;

pub use vault::{Role, Vault};

/// Tag byte that prefixes a `Vault` payload in the program's tagged `Account`
/// storage. Kept here so off-chain readers ([`Vault::from_account_data`]) and the
/// program's `Account` enum agree on the wire layout.
pub const VAULT_ACCOUNT_TAG: u8 = 1;
