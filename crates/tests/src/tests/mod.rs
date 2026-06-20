//! Integration tests, one module per instruction domain. Shared LiteSVM setup,
//! transaction submission, assertions, and the vault fixture live in
//! [`crate::helpers`].

mod action;
mod asset;
mod atomic_redeem;
mod deposit;
mod external_destination;
mod flash_approve;
mod initialize_vault;
mod manage;
mod nav_controls;
mod nav_reporting;
mod program;
mod redeem;
mod require_sibling;
mod role_authorities;
mod share_metadata;
mod swap;
mod vault_config;
mod write_down_fees;
