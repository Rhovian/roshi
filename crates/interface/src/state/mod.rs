//! Account wire types shared with off-chain readers.

pub mod vault;

pub use vault::{Role, Vault};

/// Tag byte that prefixes a `Vault` payload in the program's tagged `Account`.
pub const VAULT_ACCOUNT_TAG: u8 = 1;
