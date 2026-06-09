//! Boolean vault/asset flags stored as a single byte and validated on read.

use solana_program_error::{ProgramError, ProgramResult};

use crate::error::RoshiError;

pub const FALSE: u8 = 0;
pub const TRUE: u8 = 1;

pub const fn bool_to_flag(value: bool) -> u8 {
    if value {
        TRUE
    } else {
        FALSE
    }
}

pub fn flag_to_bool(flag: u8, error: RoshiError) -> Result<bool, ProgramError> {
    match flag {
        FALSE => Ok(false),
        TRUE => Ok(true),
        _ => Err(error.into()),
    }
}

pub fn validate_flag(flag: u8, error: RoshiError) -> ProgramResult {
    flag_to_bool(flag, error).map(|_| ())
}
