//! Integration tests, one module per instruction domain. Shared LiteSVM setup,
//! transaction submission, assertions, and the vault fixture live in
//! [`crate::helpers`].

mod action;
mod asset;
mod atomic_redeem;
mod deposit;
mod external_destination;
mod initialize_vault;
mod manage;
mod nav_reporting;
mod program;
mod redeem;
mod role_authorities;
mod swap;
mod vault_config;
mod write_down_fees;
