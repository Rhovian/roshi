use solana_account_info::AccountInfo;
use solana_program_error::ProgramResult;

pub fn try_initialize_sub_account(_accounts: &[AccountInfo], _index: u8) -> ProgramResult {
    // TODO: verify vault admin, derive [b"sub_account", vault, index], and
    // create/fund the system-owned PDA if the vault needs native SOL custody.
    // Token accounts may also use this PDA directly as their authority.
    Ok(())
}
