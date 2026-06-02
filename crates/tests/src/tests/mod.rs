//! Integration tests, one module per instruction domain. Shared LiteSVM setup,
//! transaction submission, assertions, and the vault fixture live in
//! [`crate::helpers`].

mod action;
mod asset;
mod deposit;
mod initialize_vault;
mod manage;
mod program;
mod redeem;
mod role_authorities;
mod vault_config;
