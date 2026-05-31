pub mod error;
pub mod instructions;
pub mod oracle;
#[cfg(feature = "entrypoint")]
mod processor;
pub mod state;

pub use roshi_interface::{check_id, id, ID};
