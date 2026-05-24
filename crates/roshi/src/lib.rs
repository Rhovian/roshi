pub mod error;
pub mod instructions;
#[cfg(feature = "entrypoint")]
mod processor;
pub mod state;

solana_pubkey::declare_id!("Roshi11111111111111111111111111111111111111");
