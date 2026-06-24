//! Account wire types shared with off-chain readers.

pub mod asset;
pub mod vault;

pub use asset::Asset;
pub use vault::{Role, Vault, VaultControls};

/// Tag byte that prefixes a `Vault` payload in the program's tagged `Account`.
pub const VAULT_ACCOUNT_TAG: u8 = 1;

/// Tag byte that prefixes an `Asset` payload in the program's tagged `Account`.
pub const ASSET_ACCOUNT_TAG: u8 = 4;
