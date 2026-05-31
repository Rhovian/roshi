use solana_program_error::{ProgramError, ProgramResult};

use roshi_interface::error::RoshiError;

pub(crate) const FALSE: u8 = 0;
pub(crate) const TRUE: u8 = 1;

pub(crate) const fn bool_to_flag(value: bool) -> u8 {
    if value {
        TRUE
    } else {
        FALSE
    }
}

pub(crate) fn flag_to_bool(flag: u8, error: RoshiError) -> Result<bool, ProgramError> {
    match flag {
        FALSE => Ok(false),
        TRUE => Ok(true),
        _ => Err(error.into()),
    }
}

pub(crate) fn validate_flag(flag: u8, error: RoshiError) -> ProgramResult {
    flag_to_bool(flag, error).map(|_| ())
}
